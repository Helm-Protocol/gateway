//! GET /v1/agent/:did/earnings — Graph 해자: referral earnings dashboard.
//!
//! The referral graph is the network effect moat. Every agent that you
//! onboard generates 15% of their API spend for you, permanently.
//! Their referrals generate 5%, their referrals generate 2% (depth 3).
//!
//! The billing ledger already implements this 15% split — this endpoint
//! just surfaces it as a dashboard.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Extension, Json,
};
use serde_json::json;
use std::collections::{HashMap, HashSet};

use crate::gateway::auth::CallerDid;
use crate::gateway::state::AppState;

pub async fn handle_earnings(
    State(state): State<AppState>,
    Extension(CallerDid(caller_did)): Extension<CallerDid>,
    Path(did): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    // Only allow querying your own earnings (or gateway admin)
    if did != caller_did {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "access_denied", "message": "You can only view your own earnings"})),
        ));
    }

    let api_calls = state.api_calls.read().await;

    // Depth 1: calls where I am the direct referrer
    let depth1_earnings: u64 = api_calls.iter()
        .filter(|r| r.referrer_did.as_deref() == Some(&did))
        .map(|r| r.referrer_earned)
        .sum();

    let depth1_agents: HashSet<String> = api_calls.iter()
        .filter(|r| r.referrer_did.as_deref() == Some(&did))
        .map(|r| r.caller_did.clone())
        .collect();

    // Depth 2: agents referred by MY referrals
    // (agents whose referrer is in depth1_agents, my cut = 5% of their spend)
    let depth2_callers: HashSet<String> = {
        let agents = state.agents.read().await;
        agents.values()
            .filter(|a| a.referrer_did.as_ref().map_or(false, |r| depth1_agents.contains(r)))
            .map(|a| a.did.clone())
            .collect()
    };

    let depth2_volume: u64 = api_calls.iter()
        .filter(|r| depth2_callers.contains(&r.caller_did))
        .map(|r| r.virtual_charged)
        .sum();
    let depth2_earnings = (depth2_volume as f64 * 0.05) as u64;

    // Depth 3: agents referred by depth2 agents (my cut = 2%)
    let depth3_callers: HashSet<String> = {
        let agents = state.agents.read().await;
        agents.values()
            .filter(|a| a.referrer_did.as_ref().map_or(false, |r| depth2_callers.contains(r)))
            .map(|a| a.did.clone())
            .collect()
    };

    let depth3_volume: u64 = api_calls.iter()
        .filter(|r| depth3_callers.contains(&r.caller_did))
        .map(|r| r.virtual_charged)
        .sum();
    let depth3_earnings = (depth3_volume as f64 * 0.02) as u64;

    let total_earnings = depth1_earnings + depth2_earnings + depth3_earnings;
    let tree_volume = api_calls.iter()
        .filter(|r| {
            r.referrer_did.as_deref() == Some(&did)
            || depth2_callers.contains(&r.caller_did)
            || depth3_callers.contains(&r.caller_did)
        })
        .map(|r| r.virtual_charged)
        .sum::<u64>();

    // Top 5 referrals by earnings generated
    let mut referral_earnings: HashMap<String, u64> = HashMap::new();
    for call in api_calls.iter().filter(|r| r.referrer_did.as_deref() == Some(&did)) {
        *referral_earnings.entry(call.caller_did.clone()).or_insert(0) += call.virtual_charged;
    }
    let mut top_referrals: Vec<(String, u64)> = referral_earnings.into_iter().collect();
    top_referrals.sort_by(|a, b| b.1.cmp(&a.1));
    top_referrals.truncate(5);

    drop(api_calls);

    // Get billing ledger referrer earnings
    let billing_referrer_earned = state.billing.read().await.referrer_earnings(&did);

    Ok(Json(json!({
        "did": did,
        "referral_earnings": {
            "depth1_virtual": depth1_earnings,
            "depth2_virtual": depth2_earnings,
            "depth3_virtual": depth3_earnings,
            "total_virtual": total_earnings,
            "billing_ledger_total": billing_referrer_earned,
        },
        "tree": {
            "depth1_agent_count": depth1_agents.len(),
            "depth2_agent_count": depth2_callers.len(),
            "depth3_agent_count": depth3_callers.len(),
            "total_agents_in_tree": depth1_agents.len() + depth2_callers.len() + depth3_callers.len(),
            "tree_total_volume_virtual": tree_volume,
        },
        "top_referrals": top_referrals.iter().map(|(did, vol)| {
            json!({"did": did, "volume_virtual": vol})
        }).collect::<Vec<_>>(),
        "cuts_bps": {
            "depth1": 1500,
            "depth2": 500,
            "depth3": 200,
        },
        "next_milestone": if depth1_agents.len() < 1 {
            "Refer 1 agent to start earning passive income"
        } else if depth1_agents.len() < 5 {
            "Reach 5 direct referrals to unlock Signal Channel"
        } else if depth1_agents.len() < 10 {
            "Reach 10 referrals for Elite networking bonus"
        } else {
            "Elite referrer status achieved"
        },
    })))
}

/// GET /v1/leaderboard — public referral graph leaderboard (Graph 해자 viral engine)
pub async fn handle_leaderboard(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let api_calls = state.api_calls.read().await;
    let agents = state.agents.read().await;

    // Compute referrer earnings leaderboard
    let mut earnings_map: HashMap<String, (u64, u64)> = HashMap::new(); // did -> (earnings, volume)
    for call in api_calls.iter() {
        if let Some(ref ref_did) = call.referrer_did {
            let entry = earnings_map.entry(ref_did.clone()).or_insert((0, 0));
            entry.0 += call.referrer_earned;
            entry.1 += call.virtual_charged;
        }
    }

    let mut leaderboard: Vec<serde_json::Value> = earnings_map.into_iter()
        .enumerate()
        .map(|(i, (did, (earned, volume)))| {
            let is_elite = agents.get(&did).map_or(false, |a| a.is_elite);
            json!({
                "rank": i + 1,
                "did": did,
                "earnings_virtual": earned,
                "tree_volume_virtual": volume,
                "is_elite": is_elite,
            })
        })
        .collect();

    leaderboard.sort_by(|a, b| {
        b["earnings_virtual"].as_u64().unwrap_or(0)
            .cmp(&a["earnings_virtual"].as_u64().unwrap_or(0))
    });

    // Re-rank after sort
    for (i, entry) in leaderboard.iter_mut().enumerate() {
        if let Some(obj) = entry.as_object_mut() {
            obj.insert("rank".to_string(), json!(i + 1));
        }
    }

    leaderboard.truncate(100);

    Json(json!({
        "leaderboard": leaderboard,
        "total_agents": agents.len(),
        "updated_at_ms": crate::gateway::state::now_ms(),
    }))
}
