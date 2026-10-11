//! 909-flavoured SID drum patches. (Stub until Task 9.)

use super::DrumPatch;

pub fn patch(inst: usize, tune: f32, decay: f32, tone: f32) -> DrumPatch {
    super::kit808::patch(inst, tune, decay, tone)
}
