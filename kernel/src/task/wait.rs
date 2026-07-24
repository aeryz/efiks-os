pub enum WaitStatus {
    Exited(i8),
}

#[repr(C)]
pub struct RawWaitStatus(u32);

impl From<WaitStatus> for RawWaitStatus {
    fn from(value: WaitStatus) -> Self {
        let encoded = match value {
            WaitStatus::Exited(e) => u32::from(e as u8) << 8,
        };

        Self(encoded)
    }
}
