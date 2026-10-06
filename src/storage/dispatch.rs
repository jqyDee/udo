/// Run the same call on whichever backend `self` is.
macro_rules! dispatch {
    ($self:ident, $s:ident => $call:expr) => {
        match $self {
            Self::Memory($s) => $call.await,
            Self::Sqlite($s) => $call.await,
        }
    };
}

pub(super) use dispatch;
