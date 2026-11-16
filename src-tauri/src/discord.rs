//! Discord helpers: token validation (token:validate) and the rich profile
//! fetch (discord:user) from src/main.ts.

use serde_json::{json, Value};

use crate::http::Http;

/// Public-flag badge table from main.ts.
const BADGES: &[(u64, &str, &str)] = &[
    (1, "Discord Staff", "5e74e9b61934fc1f67c65515d1f7e60d"),
    (2, "Partnered Server Owner", "3f9748e53446a137a052f3454e2f8f82"),
    (4, "HypeSquad Events", "bf01d1073931f921909045f3a39fd264"),
    (8, "Bug Hunter Level 1", "2717692c7dca7289b35297368a940dd0"),
    (64, "HypeSquad Bravery", "8a88d63823d8a71cd5e390baa45efa02"),
    (128, "HypeSquad Brilliance", "011940fd013da3f7fb926e4a1cd2e618"),
    (256, "HypeSquad Balance", "3aa41de486fa12454c3761e8e223442e"),
    (512, "Early Supporter", "7060786766c9c840eb3019e725d2b358"),
    (16384, "Bug Hunter Level 2", "848f79194d4be5ff5f81505cbd0ce1e6"),
    (131072, "Verified Bot Developer", "6df5892e0f35b051f8b61eace34f4f38"),
    (262144, "Discord Certified Moderator", "fee1624003e2fee35cb398e125dc479b"),
    (4194304, "Active Developer", "6bdc42827a38498929a4920da12695d9"),
];

fn parse_badges(flags: u64) -> Vec<(String, String)> {
    BADGES
        .iter()
        .filter(|(flag, _, _)| flags & flag != 0)
        .map(|(_, name, icon)| {
            (
                name.to_string(),
                format!("https://cdn.discordapp.com/badge-icons/{icon}.png"),
            )
        })
        .collect()
}

pub async fn validate_token(http: &Http, token: &str) -> Value {
    let resp = http
        .discord_request(
            reqwest::Method::GET,
            "https://discord.com/api/v10/users/@me",
            Some(token),
            None,
            None,
            "auth",
            1,
            15_000,
        )
        .await;

    match resp {
        Ok(r) => {
            let code = r.status().as_u16();
            if r.status().is_success() {
                json!({ "ok": true, "code": "authenticated" })
            } else if code == 401 || code == 403 {
                json!({ "ok": false, "code": "invalid_credentials" })
            } else if code == 429 {
                json!({ "ok": false, "code": "rate_limited" })
            } else {
                json!({ "ok": false, "code": "discord_unavailable" })
            }
        }
        Err(_) => json!({ "ok": false, "code": "network_error" }),
    }
}

async fn get(http: &Http, url: &str, token: &str) -> Option<Value> {
    let resp = http
        .client
        .get(url)
        .header("Authorization", token)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.json::<Value>().await.ok()
}

fn hex_color(n: i64) -> String {
    format!("#{:06x}", n & 0xff_ffff)
}

/// Port of the discord:user IPC handler in main.ts.
pub async fn user_profile(http: &Http, token: &str) -> Option<Value> {
    if token.is_empty() {
        return None;
    }

    let data = get(http, "https://discord.com/api/v10/users/@me", token).await?;

    let id = data.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let username = data.get("username").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let global_name = data
        .get("global_name")
        .and_then(|v| v.as_str())
        .unwrap_or(&username)
        .to_string();

    let avatar = data.get("avatar").and_then(|v| v.as_str());
    let avatar_ext = if avatar.map(|a| a.starts_with("a_")).unwrap_or(false) { "gif" } else { "png" };
    let banner = data.get("banner").and_then(|v| v.as_str());
    let banner_ext = if banner.map(|b| b.starts_with("a_")).unwrap_or(false) { "gif" } else { "png" };

    let public_flags = data.get("public_flags").and_then(|v| v.as_u64()).unwrap_or(0);
    let flag_badges = parse_badges(public_flags);

    // Extended profile.
    let profile_url = format!(
        "https://discord.com/api/v10/users/{id}/profile?with_mutual_guilds=false&with_mutual_friends=false"
    );
    let mut bio = String::new();
    let mut pronouns = String::new();
    let mut premium_type = 0i64;
    let mut premium_since: Value = Value::Null;
    let mut profile_badges: Vec<(String, String)> = Vec::new();
    let mut clan: Value = Value::Null;
    let mut theme_colors: Option<Vec<i64>> = None;

    if let Some(p) = get(http, &profile_url, token).await {
        let user_profile = p.get("user_profile");
        let user = p.get("user");
        bio = user_profile
            .and_then(|u| u.get("bio"))
            .and_then(|v| v.as_str())
            .or_else(|| user.and_then(|u| u.get("bio")).and_then(|v| v.as_str()))
            .unwrap_or("")
            .to_string();
        pronouns = user_profile
            .and_then(|u| u.get("pronouns"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        theme_colors = user_profile
            .and_then(|u| u.get("theme_colors"))
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_i64()).collect());
        premium_type = p.get("premium_type").and_then(|v| v.as_i64()).unwrap_or(0);
        premium_since = p.get("premium_since").cloned().unwrap_or(Value::Null);
        if let Some(badges) = p.get("badges").and_then(|v| v.as_array()) {
            for b in badges {
                let desc = b.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let icon = b.get("icon").and_then(|v| v.as_str()).unwrap_or("");
                profile_badges.push((
                    desc,
                    format!("https://cdn.discordapp.com/badge-icons/{icon}.png"),
                ));
            }
        }
        if let Some(c) = user.and_then(|u| u.get("clan")).filter(|c| !c.is_null()) {
            clan = json!({
                "tag": c.get("tag").cloned().unwrap_or(Value::Null),
                "badge": c.get("badge").cloned().unwrap_or(Value::Null),
                "guildId": c.get("identity_guild_id").cloned().unwrap_or(Value::Null),
            });
        }
    }

    // Connections.
    let connections = get(http, "https://discord.com/api/v10/users/@me/connections", token)
        .await
        .unwrap_or(Value::Array(vec![]));

    // Merge profile badges with public-flag badges (dedupe by name).
    let mut all_badges: Vec<Value> = Vec::new();
    for (name, icon) in &profile_badges {
        all_badges.push(json!({ "name": name, "icon": icon }));
    }
    for (name, icon) in &flag_badges {
        let exists = all_badges
            .iter()
            .any(|b| b.get("name").and_then(|n| n.as_str()).map(|s| s.eq_ignore_ascii_case(name)).unwrap_or(false));
        if !exists {
            all_badges.push(json!({ "name": name, "icon": icon }));
        }
    }

    let avatar_url = avatar.map(|a| {
        json!(format!(
            "https://cdn.discordapp.com/avatars/{id}/{a}.{avatar_ext}?size=256"
        ))
    }).unwrap_or(Value::Null);
    let banner_url = banner.map(|b| {
        json!(format!(
            "https://cdn.discordapp.com/banners/{id}/{b}.{banner_ext}?size=600"
        ))
    }).unwrap_or(Value::Null);

    let accent_color = data
        .get("accent_color")
        .and_then(|v| v.as_i64())
        .map(|c| json!(hex_color(c)))
        .unwrap_or(Value::Null);
    let theme_colors_val = theme_colors
        .map(|v| json!(v.iter().map(|c| hex_color(*c)).collect::<Vec<_>>()))
        .unwrap_or(Value::Null);
    let display_name_colors = data
        .get("display_name_styles")
        .and_then(|s| s.get("colors"))
        .and_then(|v| v.as_array())
        .map(|a| json!(a.iter().filter_map(|x| x.as_i64()).map(hex_color).collect::<Vec<_>>()))
        .unwrap_or(Value::Null);

    Some(json!({
        "id": id,
        "username": username,
        "globalName": global_name,
        "avatar": avatar_url,
        "banner": banner_url,
        "bannerColor": data.get("banner_color").cloned().unwrap_or(Value::Null),
        "accentColor": accent_color,
        "themeColors": theme_colors_val,
        "displayNameColors": display_name_colors,
        "badges": all_badges,
        "bio": bio,
        "pronouns": pronouns,
        "premiumType": premium_type,
        "premiumSince": premium_since,
        "clan": clan,
        "connections": connections,
    }))
}
