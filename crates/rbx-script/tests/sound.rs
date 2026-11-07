mod common;

use common::Fixture;
use std::path::PathBuf;

fn fixture() -> Fixture {
    let f = Fixture::new();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/content");
    f.engine().lock().content_root = Some(root);
    f.engine().lock().running = true;
    f
}

#[test]
fn play_pause_resume_stop_follow_the_legacy_state_machine() {
    let f = fixture();
    assert!(f.bool(
        "local s = Instance.new('Sound', workspace) s.SoundId = 'rbxasset://sounds/swoosh.wav' \
         local played, paused, stopped = 0, 0, 0 \
         s.Played:connect(function() played = played + 1 end) \
         s.Paused:connect(function() paused = paused + 1 end) \
         s.Stopped:connect(function() stopped = stopped + 1 end) \
         local before = not s.IsPlaying and s.IsPaused \
         s:Play() local a = s.IsPlaying and not s.IsPaused \
         s:Pause() local b = not s.IsPlaying and s.IsPaused \
         s:Resume() local c = s.IsPlaying \
         s:Stop() local d = not s.IsPlaying and s.TimePosition == 0 \
         return before and a and b and c and d and played == 1"
    ));
}

#[test]
fn time_length_comes_from_the_decoded_clip() {
    let f = fixture();
    let len = f.number(
        "local s = Instance.new('Sound', workspace) s.SoundId = 'rbxasset://sounds/swoosh.wav' return s.TimeLength",
    );
    assert!(len > 0.05 && len < 10.0, "length {len}");
}

#[test]
fn ended_fires_after_the_clip_finishes() {
    let f = fixture();
    f.execute(
        "local s = Instance.new('Sound', workspace) s.Name = 'S' s.SoundId = 'rbxasset://sounds/swoosh.wav' \
         _G.ended = false s.Ended:connect(function() _G.ended = true end) s:Play()",
    )
    .expect("play");
    f.step_seconds(5.0);
    assert!(f.bool("return _G.ended == true and not workspace.S.IsPlaying"));
}

#[test]
fn looped_sounds_report_did_loop() {
    let f = fixture();
    f.execute(
        "local s = Instance.new('Sound', workspace) s.Name = 'L' s.Looped = true s.SoundId = 'rbxasset://sounds/swoosh.wav' \
         _G.loops = 0 s.DidLoop:connect(function(id, n) _G.loops = n end) s:Play()",
    )
    .expect("play");
    f.step_seconds(5.0);
    assert!(f.bool("return _G.loops >= 1 and workspace.L.IsPlaying"));
}

#[test]
fn time_position_can_be_set() {
    let f = fixture();
    assert!(f.bool(
        "local s = Instance.new('Sound', workspace) s.SoundId = 'rbxasset://sounds/swoosh.wav' s.TimePosition = 0.1 return math.abs(s.TimePosition - 0.1) < 1e-6"
    ));
}

#[test]
fn listener_round_trips() {
    let f = fixture();
    assert!(f.bool(
        "local ss = game:GetService('SoundService') ss:SetListener(Enum.ListenerType.CFrame, CFrame.new(1, 2, 3)) \
         local t, v = ss:GetListener() local ok = t == Enum.ListenerType.CFrame and v.p == Vector3.new(1, 2, 3) \
         ss:SetListener(Enum.ListenerType.Camera) local t2 = ss:GetListener() return ok and t2 == Enum.ListenerType.Camera"
    ));
}
