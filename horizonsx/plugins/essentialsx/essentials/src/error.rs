//! The exceptions commands throw. Java's `throws Exception` is `EssResult` here, and the catch in
//! `onCommandEssentials` is `commands::base::dispatch`.
use crate::i18n::Arg;

#[derive(Clone, Debug)]
pub enum EssError {
    /// `net.ess3.api.TranslatableException` and all its subclasses.
    Tl { key: String, args: Vec<Arg> },
    /// `NotEnoughArgumentsException`, with its message, which is usually empty.
    NotEnoughArguments(String),
    /// `NoChargeException`: done, but the caller must not charge.
    NoCharge,
    /// `QuietAbortException`.
    QuietAbort,
    /// `new Exception(message)`.
    Message(String),
    /// `net.ess3.api.MaxMoneyException`.
    MaxMoney,
}

pub type EssResult<T = ()> = Result<T, EssError>;

/// `if (condition) throw error;`
#[macro_export]
macro_rules! fail_if {
    ($condition:expr, $error:expr) => {
        if $condition {
            return Err($error);
        }
    };
}

impl EssError {
    /// `new TranslatableException(key, args...)`.
    pub fn tl(key: &str, args: &[Arg]) -> Self {
        Self::Tl { key: key.to_owned(), args: args.to_vec() }
    }
    /// `new NotEnoughArgumentsException()`.
    pub fn not_enough() -> Self {
        Self::NotEnoughArguments(String::new())
    }
    /// `new PlayerNotFoundException()`.
    pub fn player_not_found() -> Self {
        Self::tl("playerNotFound", &[])
    }
    /// `catch (PlayerNotFoundException e)`.
    pub fn is_player_not_found(&self) -> bool {
        matches!(self, Self::Tl { key, .. } if key == "playerNotFound")
    }
    /// `new PlayerExemptException(key, args...)`.
    pub fn player_exempt(key: &str, args: &[Arg]) -> Self {
        Self::tl(key, args)
    }
    /// `new WarpNotFoundException()`.
    pub fn warp_not_found() -> Self {
        Self::tl("warpNotExist", &[])
    }
    /// `new InvalidModifierException()`.
    pub fn invalid_modifier() -> Self {
        Self::tl("invalidModifier", &[])
    }
    /// `new ChargeException(key, args...)`.
    pub fn charge(key: &str, args: &[Arg]) -> Self {
        Self::tl(key, args)
    }
    /// `getMessage()`: for a translatable one, the server's language as section sign text.
    pub fn get_message(&self) -> String {
        match self {
            Self::Tl { key, args } => crate::adventure::mini_to_legacy(&crate::i18n::tl_literal(key, args)),
            Self::NotEnoughArguments(message) | Self::Message(message) => message.clone(),
            Self::NoCharge => "Will charge later".to_owned(),
            Self::QuietAbort => String::new(),
            Self::MaxMoney => crate::adventure::mini_to_legacy(&crate::i18n::tl_literal("maxMoney", &[])),
        }
    }
}
