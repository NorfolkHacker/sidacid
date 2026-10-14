//! Tests only: the uids of the standard rack (`machine::standard_rack`),
//! which new and demo projects hold.

use acid_engine::machine::MachineUid;

pub const SYNTH1: MachineUid = MachineUid(1);
pub const SYNTH2: MachineUid = MachineUid(2);
pub const KIT808: MachineUid = MachineUid(3);
pub const KIT909: MachineUid = MachineUid(4);
pub const FM: MachineUid = MachineUid(5);
pub const SAMPLER: MachineUid = MachineUid(6);
