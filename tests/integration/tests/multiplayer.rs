use integration::{Admin, headless_client, run_lua, step_until};
use serde_json::json;
use std::time::Duration;

const WORLD: &str = r#"
local base = Instance.new("Part")
base.Name = "Baseplate"
base.Anchored = true
base.Size = Vector3.new(64, 1, 64)
base.Parent = workspace
game:GetService("NetworkServer"):Start(0)
return "started"
"#;

fn exists(path: &str) -> String {
    format!("return workspace:FindFirstChild('{path}') ~= nil")
}

#[test]
fn one_server_two_clients_replicate_both_ways() {
    let server = server::spawn(server::Config {
        http: "127.0.0.1:0".parse().unwrap(),
        game_bind: "127.0.0.1".parse().unwrap(),
        ..server::Config::default()
    })
    .unwrap();
    let admin = Admin::new(server.http);
    assert_eq!(admin.get("/v1/status")["environmentCount"], 0);
    let opened = admin.post(
        "/v1/jobs",
        json!({ "job": { "id": "game", "expirationInSeconds": 120 }, "script": { "source": WORLD } }),
    );
    assert_eq!(opened["results"][0], "started");
    let (addr, hash) = admin.wait_for_game("game", Duration::from_secs(10));

    let mut clients = vec![
        headless_client(addr, hash, "Player1", 1),
        headless_client(addr, hash, "Player2", 2),
    ];
    step_until(&mut clients, Duration::from_secs(20), |cs| {
        cs.iter().all(|c| c.joined())
            && cs.iter().all(|c| {
                run_lua(c, "return #game.Players:GetPlayers()") == ["2"]
                    && run_lua(c, &exists("Baseplate")) == ["true"]
            })
    });
    assert_eq!(
        admin.execute("game", "return #game.Players:GetPlayers()"),
        [json!(2)]
    );

    run_lua(
        &clients[0],
        "local p = Instance.new('Part') p.Name = 'FromPlayer1' p.Anchored = true p.Transparency = 0.5 p.Parent = workspace",
    );
    run_lua(
        &clients[1],
        "local p = Instance.new('Part') p.Name = 'FromPlayer2' p.Anchored = true p.Parent = workspace",
    );
    step_until(&mut clients, Duration::from_secs(20), |cs| {
        run_lua(&cs[1], &exists("FromPlayer1")) == ["true"]
            && run_lua(&cs[0], &exists("FromPlayer2")) == ["true"]
    });
    assert_eq!(
        admin.execute("game", "return workspace.FromPlayer1.Transparency, workspace:FindFirstChild('FromPlayer2') ~= nil"),
        [json!(0.5), json!(true)]
    );

    run_lua(&clients[1], "workspace.FromPlayer1.Transparency = 0.25");
    step_until(&mut clients, Duration::from_secs(20), |cs| {
        run_lua(&cs[0], "return workspace.FromPlayer1.Transparency") == ["0.25"]
    });

    admin.execute(
        "game",
        "workspace.Baseplate.BrickColor = BrickColor.new('Bright red')",
    );
    step_until(&mut clients, Duration::from_secs(20), |cs| {
        cs.iter()
            .all(|c| run_lua(c, "return workspace.Baseplate.BrickColor.Name") == ["Bright red"])
    });

    admin.delete("/v1/jobs/game");
    assert_eq!(admin.get("/v1/status")["environmentCount"], 0);
    server.stop();
}

#[test]
fn admin_api_reports_jobs_and_executes_scripts() {
    let server = server::spawn(server::Config {
        http: "127.0.0.1:0".parse().unwrap(),
        ..server::Config::default()
    })
    .unwrap();
    let admin = Admin::new(server.http);
    assert_eq!(admin.get("/v1/version")["version"], server::VERSION);
    admin.post(
        "/v1/jobs",
        json!({ "job": { "id": "a", "expirationInSeconds": 60 } }),
    );
    let r = admin.execute("a", "local t = {...} return 1 + 2, 'x', {1, 2, {k = true}}");
    assert_eq!(r, [json!(3), json!("x"), json!([1, 2, { "k": true }])]);
    let jobs = admin.get("/v1/jobs");
    assert_eq!(jobs.as_array().map(Vec::len), Some(1));
    let renewed = admin.post("/v1/jobs/a/renew", json!({ "expirationInSeconds": 300 }));
    assert!(renewed["expirationInSeconds"].as_f64().unwrap() > 250.0);
    let batch = admin.post(
        "/v1/batch",
        json!({ "job": { "id": "b" }, "script": { "source": "return ...", "arguments": [7, "y"] } }),
    );
    assert_eq!(batch["results"], json!([7, "y"]));
    assert_eq!(admin.get("/v1/status")["environmentCount"], 1);
    server.stop();
}

#[test]
fn local_web_endpoints_serve_product_info() {
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets");
    let server = server::spawn(server::Config {
        http: "127.0.0.1:0".parse().unwrap(),
        asset_dirs: vec![assets],
        ..server::Config::default()
    })
    .unwrap();
    let admin = Admin::new(server.http);
    let info = admin.get("/marketplace/productinfo?assetId=25165538");
    assert_eq!(info["AssetId"], 25165538);
    admin.post(
        "/v1/jobs",
        json!({ "job": { "id": "web", "expirationInSeconds": 60 } }),
    );
    let base = format!("http://{}/", server.http);
    let r = admin.execute(
        "web",
        &format!("game:GetService('ContentProvider'):SetBaseUrl('{base}') return game:GetService('MarketplaceService'):GetProductInfo(25165538).AssetId"),
    );
    assert_eq!(r, [json!(25165538)]);
    server.stop();
}
