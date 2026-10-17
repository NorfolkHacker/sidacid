//! `MachineType::memory_estimate` should be within 2× of what building a
//! slot actually allocates.
use acid_engine::machine::{MachineType, MachineUid};
use acid_engine::project::MachineEntry;
use acid_engine::rack::Slot;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

static BYTES: AtomicUsize = AtomicUsize::new(0);
struct Counting;
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        BYTES.fetch_add(l.size(), Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
}
#[global_allocator]
static A: Counting = Counting;

#[test]
fn estimates_are_within_two_times_of_real_allocations() {
    for ty in MachineType::ALL {
        let entry = MachineEntry::new(MachineUid(1), ty, "x".into());
        let before = BYTES.load(Ordering::Relaxed);
        let slot = Slot::from_entry(&entry, 48_000.0);
        let used = BYTES.load(Ordering::Relaxed) - before + std::mem::size_of::<Slot>();
        let est = ty.memory_estimate();
        println!("{ty:?}: estimate {est}, measured {used}");
        assert!(
            est * 2 >= used && est <= used * 2,
            "{ty:?}: estimate {est}, measured {used}"
        );
        drop(slot);
    }
}
