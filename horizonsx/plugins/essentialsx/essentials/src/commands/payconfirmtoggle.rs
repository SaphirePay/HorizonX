//! `Commandpayconfirmtoggle`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run), ..Command::new("payconfirmtoggle") };

fn run(user: &User, label: &str, _args: &[String]) -> EssResult {
    let mut confirming = !user.is_prompting_pay_confirm();
    if label.contains("payconfirmon") {
        confirming = true;
    } else if label.contains("payconfirmoff") {
        confirming = false;
    }
    user.set_prompting_pay_confirm(confirming);
    user.send_tl(if confirming { "payConfirmToggleOn" } else { "payConfirmToggleOff" }, &[]);
    user.clear_confirming_payments();
    Ok(())
}
