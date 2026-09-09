use std::cell::Cell;
use std::fs;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MemoryFloorBreach {
    pub available_kib: u64,
    pub floor_kib: u64,
}

thread_local! {
    static AVAILABLE_KIB_OVERRIDE: Cell<Option<u64>> = const { Cell::new(None) };
}

pub(crate) fn memory_floor_kib(mem_total_kib: Option<u64>) -> u64 {
    let absolute = crate::defaults::gate::MIN_MEMAVAILABLE_KIB;
    let Some(total) = mem_total_kib else {
        return absolute;
    };
    let from_percent =
        total.saturating_mul(crate::defaults::gate::MIN_MEMAVAILABLE_PERCENT) / 100;
    absolute.max(from_percent)
}

pub(crate) fn parse_meminfo_kib(meminfo: &str, key: &str) -> Option<u64> {
    for line in meminfo.lines() {
        let Some(rest) = line.strip_prefix(key) else {
            continue;
        };
        let value = rest.split_whitespace().next()?;
        return value.parse().ok();
    }
    None
}

pub(crate) fn parse_mem_available_kib(meminfo: &str) -> Option<u64> {
    parse_meminfo_kib(meminfo, "MemAvailable:")
}

pub(crate) fn check_mem_available_kib(
    available_kib: u64,
    floor_kib: u64,
) -> Result<(), MemoryFloorBreach> {
    if available_kib < floor_kib {
        return Err(MemoryFloorBreach {
            available_kib,
            floor_kib,
        });
    }
    Ok(())
}

pub(crate) fn check_host_mem_available() -> Result<(), MemoryFloorBreach> {
    let Some((available_kib, total_kib)) = current_mem_kib() else {
        return Ok(());
    };
    check_mem_available_kib(available_kib, memory_floor_kib(total_kib))
}

fn current_mem_kib() -> Option<(u64, Option<u64>)> {
    if let Some(available) = AVAILABLE_KIB_OVERRIDE.with(Cell::get) {
        return Some((available, None));
    }
    let meminfo = fs::read_to_string("/proc/meminfo").ok()?;
    Some((
        parse_mem_available_kib(&meminfo)?,
        parse_meminfo_kib(&meminfo, "MemTotal:"),
    ))
}

#[cfg(test)]
pub(crate) struct MemAvailableOverrideGuard {
    previous: Option<u64>,
}

#[cfg(test)]
impl MemAvailableOverrideGuard {
    pub(crate) fn enter(available_kib: Option<u64>) -> Self {
        let previous = AVAILABLE_KIB_OVERRIDE.with(|slot| {
            let previous = slot.get();
            slot.set(available_kib);
            previous
        });
        Self { previous }
    }
}

#[cfg(test)]
impl Drop for MemAvailableOverrideGuard {
    fn drop(&mut self) {
        AVAILABLE_KIB_OVERRIDE.with(|slot| slot.set(self.previous));
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MemoryFloorBreach, check_mem_available_kib, memory_floor_kib, parse_mem_available_kib,
        parse_meminfo_kib,
    };

    #[test]
    fn parse_mem_available_kib_reads_kb_value() {
        let meminfo = "MemTotal: 131072000 kB\nMemAvailable: 4096 kB\n";
        assert_eq!(parse_mem_available_kib(meminfo), Some(4096));
        assert_eq!(parse_meminfo_kib(meminfo, "MemTotal:"), Some(131_072_000));
    }

    #[test]
    fn parse_mem_available_kib_ignores_other_keys() {
        assert_eq!(parse_mem_available_kib("MemFree: 1 kB\n"), None);
    }

    #[test]
    fn check_mem_available_kib_is_hard_error_below_floor() {
        let err = check_mem_available_kib(10, 100).unwrap_err();
        assert_eq!(
            err,
            MemoryFloorBreach {
                available_kib: 10,
                floor_kib: 100
            }
        );
    }

    #[test]
    fn check_mem_available_kib_allows_floor_and_above() {
        check_mem_available_kib(100, 100).unwrap();
        check_mem_available_kib(101, 100).unwrap();
    }

    #[test]
    fn documented_floor_matches_defaults() {
        assert_eq!(
            memory_floor_kib(None),
            crate::defaults::gate::MIN_MEMAVAILABLE_KIB
        );
        assert_eq!(memory_floor_kib(None), 262_144);
        assert_eq!(memory_floor_kib(Some(1_000_000)), 262_144);
        assert_eq!(memory_floor_kib(Some(131_072_000)), 13_107_200);
    }
}
