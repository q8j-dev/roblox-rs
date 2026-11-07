mod common;

use common::Fixture;

#[test]
fn json_round_trip() {
    let f = Fixture::new();
    assert!(f.bool(
        "local h = game:GetService('HttpService') local s = h:JSONEncode({a = 1, b = {true, 'x'}}) local t = h:JSONDecode(s) return t.a == 1 and t.b[1] == true and t.b[2] == 'x'"
    ));
    assert!(f.bool(
        "local h = game:GetService('HttpService') return not pcall(function() h:JSONEncode(5) end) and not pcall(function() h:JSONDecode('{') end)"
    ));
}

#[test]
fn url_encode_and_guid() {
    let f = Fixture::new();
    assert!(f.bool("return game:GetService('HttpService'):UrlEncode('a b&c') == 'a%20b%26c'"));
    assert!(f.bool(
        "local h = game:GetService('HttpService') local g = h:GenerateGUID() local p = h:GenerateGUID(false) return #g == 38 and g:sub(1, 1) == '{' and #p == 36 and g ~= h:GenerateGUID()"
    ));
}

#[test]
fn requests_need_http_enabled() {
    let f = Fixture::new();
    assert!(f.bool(
        "local ok, err = pcall(function() return game:GetService('HttpService'):GetAsync('http://localhost:1/') end) return not ok and err:find('not enabled') ~= nil"
    ));
}
