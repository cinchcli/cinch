//! `cinch plan` — show the caller's plan tier + active usage.
//!
//! Read-only: there is no `cinch plan set`. Plan changes flow through ops
//! (operator runbook in Phase 5 docs); this command just renders whatever
//! `MeService.GetMe` reports.

use client_core::rest::GetMeResponse;

use crate::exit::{ExitError, AUTH_FAILURE, GENERIC_ERROR};

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Print machine-readable JSON instead of the human table.
    #[arg(long)]
    pub json: bool,
}

pub async fn run(args: Args) -> Result<(), ExitError> {
    let ctx = crate::runtime::open_ctx().map_err(|_| {
        ExitError::new(
            AUTH_FAILURE,
            "No auth token configured.",
            "Run: cinch auth login",
        )
    })?;

    let me = ctx.client.get_me().await.map_err(ExitError::from)?;

    if args.json {
        let json = serde_json::to_string_pretty(&me)
            .map_err(|e| ExitError::new(GENERIC_ERROR, format!("serializing plan: {e}"), ""))?;
        println!("{}", json);
        return Ok(());
    }

    println!("{}", format_plan_table(&me));
    Ok(())
}

fn format_plan_table(me: &GetMeResponse) -> String {
    let plan = me.plan.as_ref();
    let usage = me.usage.as_ref();

    let device_limit = plan.map(|p| p.device_limit).unwrap_or(0);
    let retention_days = plan.map(|p| p.retention_days).unwrap_or(0);
    let active = usage.map(|u| u.active_devices).unwrap_or(0);

    let device_cap = if device_limit == 0 {
        "unlimited".to_string()
    } else {
        device_limit.to_string()
    };
    let retention_cap = if retention_days == 0 {
        "unlimited".to_string()
    } else {
        format!("{} days", retention_days)
    };

    let plan_name = if me.plan_name.is_empty() {
        "free"
    } else {
        me.plan_name.as_str()
    };

    format!(
        "Plan:            {}\nDevices:         {} / {}\nRelay retention: {}",
        plan_name, active, device_cap, retention_cap,
    )
}

#[cfg(test)]
mod tests {
    use super::format_plan_table;
    use client_core::rest::{GetMeResponse, Plan, Usage};

    #[test]
    fn renders_free_plan_with_partial_usage() {
        let me = GetMeResponse {
            plan_name: "free".into(),
            plan: Some(Plan {
                device_limit: 3,
                retention_days: 7,
                rate_limit: 0,
            }),
            usage: Some(Usage { active_devices: 2 }),
        };
        let out = format_plan_table(&me);
        assert!(out.contains("Plan:            free"), "out: {out}");
        assert!(out.contains("Devices:         2 / 3"), "out: {out}");
        assert!(out.contains("Relay retention: 7 days"), "out: {out}");
    }

    #[test]
    fn renders_unlimited_caps() {
        let me = GetMeResponse {
            plan_name: "team".into(),
            plan: Some(Plan {
                device_limit: 0,
                retention_days: 0,
                rate_limit: 0,
            }),
            usage: Some(Usage { active_devices: 5 }),
        };
        let out = format_plan_table(&me);
        assert!(out.contains("Plan:            team"), "out: {out}");
        assert!(out.contains("5 / unlimited"), "out: {out}");
        assert!(out.contains("Relay retention: unlimited"), "out: {out}");
    }

    #[test]
    fn renders_empty_plan_name_as_free_fallback() {
        // Relay may legitimately omit plan_name (Go omitempty on empty
        // string); we fall back to "free" so the user sees something
        // meaningful rather than a blank field.
        let me = GetMeResponse {
            plan_name: String::new(),
            plan: None,
            usage: None,
        };
        let out = format_plan_table(&me);
        assert!(out.contains("Plan:            free"), "out: {out}");
        assert!(out.contains("Devices:         0 / unlimited"), "out: {out}");
        assert!(out.contains("Relay retention: unlimited"), "out: {out}");
    }
}
