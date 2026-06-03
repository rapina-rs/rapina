pub mod env;
pub mod urls;

use std::sync::LazyLock;

pub(crate) static AGENT: LazyLock<ureq::Agent> = LazyLock::new(ureq::Agent::new_with_defaults);
