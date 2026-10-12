use std::time::Duration;

/// What a `go` command allows the search to spend.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Limits {
    pub depth: Option<u32>,
    pub movetime: Option<Duration>,
    /// wtime and btime, indexed by color
    pub time: [Option<Duration>; 2],
    /// winc and binc, indexed by color
    pub inc: [Duration; 2],
    pub movestogo: Option<u32>,
}
