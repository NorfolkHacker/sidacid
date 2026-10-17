// The meter (Task 9) uses these.
#![allow(dead_code)]
//! System RAM figures for the memory meter, behind a trait so tests can
//! fake a machine that is running short.

pub const MIB: u64 = 1 << 20;
pub const GIB: u64 = 1 << 30;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MemSnapshot {
    pub total: u64,
    pub available: u64,
}

pub trait SystemMemory {
    fn snapshot(&mut self) -> Option<MemSnapshot>;
}

pub struct SysinfoMemory {
    sys: sysinfo::System,
}

impl SysinfoMemory {
    pub fn new() -> Self {
        Self { sys: sysinfo::System::new() }
    }
}

impl SystemMemory for SysinfoMemory {
    fn snapshot(&mut self) -> Option<MemSnapshot> {
        self.sys.refresh_memory();
        let total = self.sys.total_memory();
        (total > 0).then(|| MemSnapshot {
            total,
            available: self.sys.available_memory().min(total),
        })
    }
}

#[cfg(test)]
pub struct FakeMemory(pub Option<MemSnapshot>);

#[cfg(test)]
impl SystemMemory for FakeMemory {
    fn snapshot(&mut self) -> Option<MemSnapshot> {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemLevel {
    Normal,
    Low,
    Critical,
}

/// Amber below 25 % of total or 2 GB, red below 10 % or 512 MB
/// (whichever is larger in each case).
pub fn level(total: u64, available: u64) -> MemLevel {
    let low = (total / 4).max(2 * GIB);
    let critical = (total / 10).max(512 * MIB);
    if available < critical {
        MemLevel::Critical
    } else if available < low {
        MemLevel::Low
    } else {
        MemLevel::Normal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_follow_the_spec_thresholds() {
        let t = 16 * GIB;
        // 25 % of 16 GB = 4 GB > 2 GB; 10 % = 1.6 GB > 512 MB.
        assert_eq!(level(t, 5 * GIB), MemLevel::Normal);
        assert_eq!(level(t, 4 * GIB - 1), MemLevel::Low);
        assert_eq!(level(t, 1_600 * MIB), MemLevel::Critical);
        // Small machine: 4 GB total -> floors 2 GB and 512 MB win.
        let s = 4 * GIB;
        assert_eq!(level(s, 2 * GIB + 1), MemLevel::Normal);
        assert_eq!(level(s, 2 * GIB - 1), MemLevel::Low);
        assert_eq!(level(s, 512 * MIB - 1), MemLevel::Critical);
    }

    #[test]
    fn sysinfo_reports_something_sane() {
        let s = SysinfoMemory::new().snapshot().expect("this OS reports memory");
        assert!(s.total > 256 * MIB && s.available <= s.total);
    }
}
