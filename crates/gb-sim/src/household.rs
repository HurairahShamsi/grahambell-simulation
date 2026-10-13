//! Household downtime (SPEC §4.7; architect's M1.2 answer Q3; ASSUMPTIONS SIM entries).
//!
//! A household's node is online when none of its outage causes is active. Each cause is a
//! process with its own keyed random stream:
//!
//! | Cause | Kind | Announced |
//! |---|---|---|
//! | Night-time shutdown (laptop profile) | daily schedule in local time | yes |
//! | Other shutdowns (updates, user) | Poisson, log-normal length | yes |
//! | Crash | Poisson, log-normal length | no |
//! | Network glitch | Poisson, log-normal length | no |
//! | Long absence | Poisson, Lomax length | by share |
//! | Permanent departure | once, exponential time | no |
//! | Power cut (feeder group), provider outage, regional blackout | shared, see `world` | no |
//!
//! A node shutting down normally sends its own offline request first (SPEC §4.5 client
//! requirement), so only crashes, glitches, power and network failures and unannounced
//! absences leave it recorded online.

use crate::engine::{Micros, SECOND, micros};
use crate::rng::{Keyed, Purpose, unit};
use crate::samplers::{exponential, lognormal, lomax, normal};
use gb_config::sim::{DeviceProfile, SimHouseholds};

/// Outage causes, as bits of a household's down mask.
pub mod bit {
    /// Night-time shutdown.
    pub const NIGHT: u16 = 1 << 0;
    /// Other announced shutdowns.
    pub const USER: u16 = 1 << 1;
    /// Crash.
    pub const CRASH: u16 = 1 << 2;
    /// Network glitch.
    pub const GLITCH: u16 = 1 << 3;
    /// Long absence.
    pub const ABSENCE: u16 = 1 << 4;
    /// Permanent departure.
    pub const DEPART: u16 = 1 << 5;
    /// Power cut of the feeder group (fault or scheduled load-shedding).
    pub const GRID: u16 = 1 << 6;
    /// Internet-provider outage.
    pub const ISP: u16 = 1 << 7;
    /// Regional blackout.
    pub const BLACKOUT: u16 = 1 << 8;
    /// Kept offline by the experiment (an attacker that leaves, or a scripted crash).
    pub const SCRIPT: u16 = 1 << 9;
    /// Scheduled load-shedding of the feeder group.
    pub const LOADSHED: u16 = 1 << 10;
    /// Number of cause bits.
    pub const COUNT: usize = 11;
}

/// The household's own processes, in the order of [`Household::next`].
pub const OWN_CAUSES: [u16; 6] = [
    bit::NIGHT,
    bit::USER,
    bit::CRASH,
    bit::GLITCH,
    bit::ABSENCE,
    bit::DEPART,
];

/// Device profiles, indexing `SimHouseholds::device_mix`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Device {
    /// Always on.
    AlwaysOn = 0,
    /// Desktop left on.
    DesktopOn = 1,
    /// Laptop or PC off at night.
    LaptopNight = 2,
}

impl Device {
    /// From the index in the device mix.
    pub fn from_index(i: usize) -> Device {
        match i {
            0 => Device::AlwaysOn,
            1 => Device::DesktopOn,
            _ => Device::LaptopNight,
        }
    }

    /// Name used in tables.
    pub fn name(self) -> &'static str {
        match self {
            Device::AlwaysOn => "always_on",
            Device::DesktopOn => "desktop_on",
            Device::LaptopNight => "laptop_night",
        }
    }

    /// The profile of this device in the configuration.
    pub fn profile(self, h: &SimHouseholds) -> &DeviceProfile {
        match self {
            Device::AlwaysOn => &h.always_on,
            Device::DesktopOn => &h.desktop_on,
            Device::LaptopNight => &h.laptop_night,
        }
    }
}

/// One household: a node that may host a miner slot and witness IDs.
#[derive(Debug, Clone)]
pub struct Household {
    /// Region index.
    pub region: u8,
    /// Device profile.
    pub device: Device,
    /// Global feeder-group index (power cuts are shared within it).
    pub grid_group: u32,
    /// Global provider index (provider outages are shared within it).
    pub isp: u32,
    /// Has a UPS or inverter.
    pub ups: bool,
    /// Clock offset from true time, seconds (positive: the clock runs ahead).
    pub skew: f64,
    /// One-way access (last-mile) delay, seconds.
    pub access: f64,
    /// Active outage causes.
    pub mask: u16,
    /// Causes whose current outage was announced.
    pub announced: u16,
    /// When each cause's current outage ends (`u64::MAX` when unknown or permanent).
    pub ends: [Micros; bit::COUNT],
    /// Next transition of each own cause.
    pub next: [Micros; 6],
    /// Draws used by each own cause.
    pub draws: [u32; 6],
    /// Generation of the household's scheduled wake (stale queue entries are ignored).
    pub wake_gen: u32,
    /// The miner slot it hosts, if any.
    pub miner: Option<u32>,
    /// Witness IDs it hosts.
    pub witnesses: Vec<u32>,
    /// Exempt from outages (experiments that keep some nodes always online).
    pub always_online: bool,
    /// C7 check: instead of the profiles, the node alternates between exponential up and down
    /// periods with these means (seconds), so it is online with probability up/(up + down)
    /// independently of every other node.
    pub independent: Option<(f64, f64)>,
}

impl Household {
    /// True when no outage cause is active.
    pub fn online(&self) -> bool {
        self.mask == 0
    }

    /// When the household will be back online if no new cause starts (now if online).
    pub fn back_at(&self, now: Micros) -> Micros {
        if self.mask == 0 {
            return now;
        }
        (0..bit::COUNT)
            .filter(|b| self.mask & (1 << b) != 0)
            .map(|b| self.ends[b])
            .max()
            .unwrap_or(now)
    }

    /// The earliest scheduled transition of the household's own causes.
    pub fn next_wake(&self) -> Micros {
        self.next.iter().copied().min().unwrap_or(u64::MAX)
    }
}

/// The outcome of one transition of an own cause.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transition {
    /// The cause.
    pub cause: u16,
    /// Starting (true) or ending (false).
    pub start: bool,
    /// Whether a starting outage is announced.
    pub announced: bool,
    /// When a starting outage ends.
    pub ends: Micros,
}

fn day_us() -> Micros {
    86_400 * SECOND
}

/// Context the own-cause processes need.
#[derive(Debug, Clone, Copy)]
pub struct CauseContext<'a> {
    /// Household settings.
    pub settings: &'a SimHouseholds,
    /// Local offset of the household's region from UTC, hours.
    pub utc_offset_h: f64,
    /// Seconds per year.
    pub seconds_per_year: f64,
}

/// The first scheduled transition of each own cause for household `h`.
pub fn initial_next(keyed: &mut Keyed, h: u64, hh: &mut Household, ctx: &CauseContext<'_>) {
    for (c, cause) in OWN_CAUSES.iter().enumerate() {
        hh.next[c] = next_start(keyed, h, hh, c, *cause, 0, ctx);
    }
}

fn draw(keyed: &mut Keyed, h: u64, hh: &mut Household, c: usize) -> [f64; 8] {
    let n = hh.draws[c];
    hh.draws[c] += 1;
    keyed.uniforms(Purpose::Household, (h << 4) | c as u64, u64::from(n))
}

/// Local time `hour` on the day containing `t` (or the next day when already passed), with
/// a normal jitter, in simulated time (time 0 is midnight UTC).
fn next_local_hour(t: Micros, hour: f64, utc_offset_h: f64, jitter: f64) -> Micros {
    let day = day_us();
    let base_hour = (hour - utc_offset_h).rem_euclid(24.0);
    let start_of_day = t - t % day;
    let mut candidate = start_of_day + micros(base_hour * 3_600.0);
    if candidate <= t {
        candidate += day;
    }
    let jittered = candidate as f64 + jitter * 3_600.0 * SECOND as f64;
    if jittered <= t as f64 {
        t + 1
    } else {
        jittered as Micros
    }
}

fn next_start(
    keyed: &mut Keyed,
    h: u64,
    hh: &mut Household,
    c: usize,
    cause: u16,
    now: Micros,
    ctx: &CauseContext<'_>,
) -> Micros {
    if let Some((up, _)) = hh.independent {
        if cause != bit::GLITCH {
            return u64::MAX;
        }
        let u = draw(keyed, h, hh, c);
        return now.saturating_add(micros(exponential(1.0 - u[0], 1.0 / up)));
    }
    if hh.always_online {
        return u64::MAX;
    }
    let p = hh.device.profile(ctx.settings).clone();
    let day = 86_400.0;
    let rate_per_s = match cause {
        bit::NIGHT => {
            if p.night_off_prob <= 0.0 {
                return u64::MAX;
            }
            let u = draw(keyed, h, hh, c);
            let jitter = p.jitter_h * normal(1.0 - u[0], u[1]);
            return next_local_hour(now, p.off_start_h, ctx.utc_offset_h, jitter);
        }
        bit::USER => p.user_off_per_day / day,
        bit::CRASH => p.crashes_per_day / day,
        bit::GLITCH => p.glitches_per_day / day,
        bit::ABSENCE => ctx.settings.long_absences_per_year / ctx.seconds_per_year,
        bit::DEPART => {
            if hh.draws[c] > 0 {
                return u64::MAX;
            }
            ctx.settings.departures_per_year / ctx.seconds_per_year
        }
        _ => 0.0,
    };
    if rate_per_s <= 0.0 {
        return u64::MAX;
    }
    let u = draw(keyed, h, hh, c);
    now.saturating_add(micros(exponential(1.0 - u[0], rate_per_s)))
}

/// Applies the due transition of own cause `c` at `now` and schedules its next one.
pub fn transition(
    keyed: &mut Keyed,
    h: u64,
    hh: &mut Household,
    c: usize,
    now: Micros,
    ctx: &CauseContext<'_>,
) -> Option<Transition> {
    let cause = OWN_CAUSES[c];
    let active = hh.mask & cause != 0;
    if active {
        // The outage ends; draw the next start.
        hh.next[c] = next_start(keyed, h, hh, c, cause, now, ctx);
        return Some(Transition {
            cause,
            start: false,
            announced: false,
            ends: now,
        });
    }
    let p = hh.device.profile(ctx.settings).clone();
    let u = draw(keyed, h, hh, c);
    if let Some((_, down)) = hh.independent {
        let ends = now.saturating_add(micros(exponential(1.0 - u[0], 1.0 / down).max(1.0)));
        hh.next[c] = ends;
        return Some(Transition {
            cause,
            start: true,
            announced: false,
            ends,
        });
    }
    let (length_s, announced) = match cause {
        bit::NIGHT => {
            if u[2] >= p.night_off_prob {
                // Left on tonight: schedule tomorrow's decision.
                hh.next[c] = next_start(keyed, h, hh, c, cause, now + 60 * SECOND, ctx);
                return None;
            }
            let jitter = p.jitter_h * normal(1.0 - u[0], u[1]);
            let end = next_local_hour(now, p.off_end_h, ctx.utc_offset_h, jitter);
            ((end - now) as f64 / SECOND as f64, true)
        }
        bit::USER => (
            3_600.0
                * lognormal(
                    p.user_off_median_h,
                    p.user_off_sigma,
                    normal(1.0 - u[0], u[1]),
                ),
            true,
        ),
        bit::CRASH => (
            60.0 * lognormal(p.crash_median_min, p.crash_sigma, normal(1.0 - u[0], u[1])),
            false,
        ),
        bit::GLITCH => (
            lognormal(p.glitch_median_s, p.glitch_sigma, normal(1.0 - u[0], u[1])),
            false,
        ),
        bit::ABSENCE => (
            86_400.0
                * lomax(
                    1.0 - u[0],
                    ctx.settings.long_absence_scale_days,
                    ctx.settings.long_absence_shape,
                ),
            u[3] < ctx.settings.long_absence_announced,
        ),
        bit::DEPART => (f64::INFINITY, false),
        _ => return None,
    };
    let ends = if length_s.is_finite() {
        now.saturating_add(micros(length_s.max(1.0)))
    } else {
        u64::MAX
    };
    hh.next[c] = ends;
    Some(Transition {
        cause,
        start: true,
        announced,
        ends,
    })
}

/// The scheduled load-shedding blocks of a feeder group with `hours` hours a day: the hours
/// are split into equal blocks of at most `block_h` hours, spread evenly over the day from
/// `phase_h` (UTC). Returns `(start, length)` in seconds within a day.
pub fn loadshed_blocks(hours: f64, block_h: f64, phase_h: f64) -> Vec<(f64, f64)> {
    if hours <= 0.0 {
        return Vec::new();
    }
    let blocks = (hours / block_h).ceil().max(1.0) as usize;
    let length = hours / blocks as f64 * 3_600.0;
    (0..blocks)
        .map(|i| {
            let start = (phase_h + i as f64 * 24.0 / blocks as f64).rem_euclid(24.0) * 3_600.0;
            (start, length)
        })
        .collect()
}

/// A uniform index in `0..n` from a word.
pub fn pick(word: u64, n: usize) -> usize {
    ((unit(word) * n as f64) as usize).min(n.saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn household(device: Device) -> Household {
        Household {
            region: 3,
            device,
            grid_group: 0,
            isp: 0,
            ups: false,
            skew: 0.0,
            access: 0.01,
            mask: 0,
            announced: 0,
            ends: [u64::MAX; bit::COUNT],
            next: [u64::MAX; 6],
            draws: [0; 6],
            wake_gen: 0,
            miner: None,
            witnesses: Vec::new(),
            always_online: false,
            independent: None,
        }
    }

    /// Runs one household's own causes for `days` and returns the offline fraction and the
    /// announced share of outages.
    fn simulate(device: Device, days: u64) -> (f64, f64, u64) {
        let settings = SimHouseholds::default();
        let ctx = CauseContext {
            settings: &settings,
            utc_offset_h: 0.0,
            seconds_per_year: 31_536_000.0,
        };
        let mut keyed = Keyed::new(5, "household-test");
        let mut hh = household(device);
        initial_next(&mut keyed, 1, &mut hh, &ctx);
        let end = days * day_us();
        let (mut down, mut since, mut outages, mut announced) = (0u64, 0u64, 0u64, 0u64);
        loop {
            let (c, t) = hh
                .next
                .iter()
                .enumerate()
                .min_by_key(|(_, t)| **t)
                .map(|(c, t)| (c, *t))
                .unwrap();
            if t >= end {
                break;
            }
            let was_online = hh.online();
            if let Some(tr) = transition(&mut keyed, 1, &mut hh, c, t, &ctx) {
                if tr.start {
                    hh.mask |= tr.cause;
                    hh.ends[c] = tr.ends;
                    if was_online {
                        since = t;
                        outages += 1;
                        announced += u64::from(tr.announced);
                    }
                } else {
                    hh.mask &= !tr.cause;
                    if hh.online() {
                        down += t - since;
                    }
                }
            }
        }
        (
            down as f64 / end as f64,
            announced as f64 / outages.max(1) as f64,
            outages,
        )
    }

    #[test]
    fn a_laptop_is_off_about_a_third_of_the_time_and_announces_most_outages() {
        let (off, announced, _) = simulate(Device::LaptopNight, 400);
        // Nights of 8 hours on 90 % of days (0.3), plus daytime shutdowns and absences.
        assert!((0.3..0.48).contains(&off), "{off}");
        assert!(announced > 0.6, "{announced}");
    }

    #[test]
    fn an_always_on_device_is_rarely_off() {
        let (off, _, outages) = simulate(Device::AlwaysOn, 400);
        assert!(off < 0.08, "{off}");
        assert!(outages > 40, "{outages}");
    }

    #[test]
    fn load_shedding_blocks_are_spread_over_the_day() {
        let b = loadshed_blocks(6.0, 2.0, 1.0);
        assert_eq!(b.len(), 3);
        assert_eq!(b[0], (3_600.0, 7_200.0));
        assert_eq!(b[1].0, 9.0 * 3_600.0);
        assert!(loadshed_blocks(0.0, 2.0, 0.0).is_empty());
        assert_eq!(loadshed_blocks(12.0, 2.0, 0.0).len(), 6);
    }

    #[test]
    fn local_hours_follow_the_utc_offset() {
        // 23:00 local at UTC+1 is 22:00 UTC.
        let t = next_local_hour(0, 23.0, 1.0, 0.0);
        assert_eq!(t, 22 * 3_600 * SECOND);
        // Already past today: tomorrow.
        let t2 = next_local_hour(23 * 3_600 * SECOND, 23.0, 1.0, 0.0);
        assert_eq!(t2, (24 + 22) * 3_600 * SECOND);
    }
}
