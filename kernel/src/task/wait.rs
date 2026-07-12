pub enum WaitStatus {
    Exited(i8),
}

#[repr(C)]
pub struct RawWaitStatus(i16);

impl From<WaitStatus> for RawWaitStatus {
    fn from(value: WaitStatus) -> Self {
        let encoded = match value {
            WaitStatus::Exited(e) => (e as i16) << 8,
        };

        Self(encoded)
    }
}
