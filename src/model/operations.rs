#[derive(Clone, Debug, Default)]
pub struct ProgressSnapshot {
    pub percent: f32,
    pub item: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum OperationError {
    Cancelled,
    Failed(String),
}
