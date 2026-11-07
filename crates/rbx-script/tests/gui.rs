mod common;

use common::Fixture;
use mlua::Value;
use rbx_script::Identity;

fn core_bool(f: &Fixture, src: &str) -> bool {
    match f.execute_as(src, Identity::RobloxGameScript) {
        Ok(v) => matches!(v.first(), Some(Value::Boolean(true))),
        Err(e) => panic!("{src}: {e}"),
    }
}

#[test]
fn absolute_placement_follows_udim2() {
    let f = Fixture::new();
    assert!(f.bool(
        "local sg = Instance.new('ScreenGui', game:GetService('StarterGui')) local fr = Instance.new('Frame', sg) fr.Size = UDim2.new(0.5, 10, 0, 20) fr.Position = UDim2.new(0, 5, 0.5, 0) local c = Instance.new('Frame', fr) c.Size = UDim2.new(1, 0, 1, 0) c.SizeConstraint = Enum.SizeConstraint.RelativeYY return fr.AbsoluteSize == Vector2.new(410, 20) and fr.AbsolutePosition == Vector2.new(5, 300) and c.AbsoluteSize == Vector2.new(20, 20) and sg.AbsoluteSize == Vector2.new(800, 600)"
    ));
}

#[test]
fn gui_inset_shifts_viewport() {
    let f = Fixture::new();
    assert!(core_bool(
        &f,
        "game:GetService('GuiService'):SetGlobalGuiInset(0, 36, 0, 0) local sg = Instance.new('ScreenGui', game:GetService('StarterGui')) local fr = Instance.new('Frame', sg) fr.Size = UDim2.new(1, 0, 1, 0) return fr.AbsoluteSize == Vector2.new(800, 564) and fr.AbsolutePosition == Vector2.new(0, 0)"
    ));
}

#[test]
fn text_size_measures_text() {
    let f = Fixture::new();
    assert!(core_bool(
        &f,
        "local ts = game:GetService('TextService') local a = ts:GetTextSize('Hello', 18, Enum.Font.SourceSans, Vector2.new(0, 0)) local b = ts:GetTextSize('Hello world', 18, Enum.Font.SourceSans, Vector2.new(50, 100)) return a.Y == 18 and a.X > 20 and b.Y == 36"
    ));
}

#[test]
fn core_gui_settings() {
    let f = Fixture::new();
    assert!(core_bool(
        &f,
        "local sg = game:GetService('StarterGui') sg:SetCoreGuiEnabled(Enum.CoreGuiType.All, false) local off = not sg:GetCoreGuiEnabled(Enum.CoreGuiType.Chat) sg:SetCoreGuiEnabled(Enum.CoreGuiType.Chat, true) local got = nil sg:RegisterSetCore('Thing', function(v) got = v end) sg:SetCore('Thing', 5) return off and sg:GetCoreGuiEnabled(Enum.CoreGuiType.Chat) and got == 5 and not pcall(function() sg:SetCore('Missing', 1) end)"
    ));
}
