use super::{Registry, Ret, none, ret};
use crate::convert::{arg_bool, arg_instance, arg_number, arg_string};
use crate::runtime::{
    MessageType, ThreadInfo, engine, new_environment, resume_thread, thread_key, with_state,
};
use crate::security::Identity;
use crate::types::rt;
use mlua::{Lua, MultiValue, Result, Value};
use rbx_dom::{InstanceId, Variant};

#[derive(Clone, Debug, Default)]
pub struct TestState {
    pub running: bool,
    pub run_count: u32,
    pub tests: u32,
    pub warns: u32,
    pub errors: u32,
    pub scripts: u32,
    pub done: bool,
    pub deadline: f64,
    pub failures: Vec<String>,
    pub warnings: Vec<String>,
    pub messages: Vec<String>,
}

fn test_service(lua: &Lua) -> Option<InstanceId> {
    engine(lua).lock().dm.find_service("TestService")
}

fn output(lua: &Lua, kind: MessageType, source: Option<InstanceId>, line: i64, message: &str) {
    let eng = engine(lua);
    let mut e = eng.lock();
    let text = match source {
        Some(s) => format!("TestService.{}({}): {}", e.dm.name_of(s), line, message),
        None => format!("TestService: {message}"),
    };
    e.print(kind, text);
}

fn bump(lua: &Lua, error: bool, warn: bool) {
    let eng = engine(lua);
    let mut e = eng.lock();
    e.test.tests += 1;
    if error {
        e.test.errors += 1;
    }
    if warn {
        e.test.warns += 1;
    }
    let (t, w, er) = (e.test.tests, e.test.warns, e.test.errors);
    if let Some(ts) = e.dm.find_service("TestService") {
        e.dm.store_silent(ts, "TestCount", Variant::Int(t as i32));
        e.dm.store_silent(ts, "WarnCount", Variant::Int(w as i32));
        e.dm.store_silent(ts, "ErrorCount", Variant::Int(er as i32));
    }
}

fn record_failure(lua: &Lua, text: String) {
    engine(lua).lock().test.failures.push(text);
}

fn parse_common(lua: &Lua, args: &[Value], offset: usize) -> (Option<InstanceId>, i64) {
    let source = arg_instance(lua, args.get(offset));
    let line = arg_number(args.get(offset + 1)).unwrap_or(0.0) as i64;
    (source, line)
}

fn check(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let condition = arg_bool(args.first());
    let description = arg_string(args.get(1)).unwrap_or_default();
    let (source, line) = parse_common(lua, args, 2);
    bump(lua, !condition, false);
    if !condition {
        output(lua, MessageType::Error, source, line, &description);
        record_failure(lua, description);
    }
    none()
}

fn warn(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let condition = arg_bool(args.first());
    let description = arg_string(args.get(1)).unwrap_or_default();
    let (source, line) = parse_common(lua, args, 2);
    bump(lua, false, !condition);
    if !condition {
        output(lua, MessageType::Warning, source, line, &description);
        engine(lua).lock().test.warnings.push(description);
    }
    none()
}

fn require(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let condition = arg_bool(args.first());
    let description = arg_string(args.get(1)).unwrap_or_default();
    let (source, line) = parse_common(lua, args, 2);
    bump(lua, !condition, false);
    if !condition {
        output(
            lua,
            MessageType::Error,
            source,
            line,
            &format!("fatal {description}"),
        );
        record_failure(lua, description);
        done(lua, id, &[])?;
    }
    none()
}

fn error(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let description = arg_string(args.first()).unwrap_or_default();
    let (source, line) = parse_common(lua, args, 1);
    bump(lua, true, false);
    output(lua, MessageType::Error, source, line, &description);
    record_failure(lua, description);
    none()
}

fn fail(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let description = arg_string(args.first()).unwrap_or_default();
    let (source, line) = parse_common(lua, args, 1);
    bump(lua, true, false);
    output(
        lua,
        MessageType::Error,
        source,
        line,
        &format!("fatal {description}"),
    );
    record_failure(lua, description);
    done(lua, id, &[])?;
    none()
}

fn message(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let text = arg_string(args.first()).unwrap_or_default();
    let (source, line) = parse_common(lua, args, 1);
    output(lua, MessageType::Info, source, line, &text);
    engine(lua).lock().test.messages.push(text);
    none()
}

fn checkpoint(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let text = arg_string(args.first()).unwrap_or_default();
    let (source, line) = parse_common(lua, args, 1);
    output(
        lua,
        MessageType::Info,
        source,
        line,
        &format!("checkpoint {text}"),
    );
    none()
}

pub fn finish(lua: &Lua) {
    let (was, tests, warns, errors) = {
        let eng = engine(lua);
        let mut e = eng.lock();
        let was = e.test.running;
        e.test.running = false;
        e.test.done = true;
        (was, e.test.tests, e.test.warns, e.test.errors)
    };
    if !was {
        return;
    }
    if tests == 0 {
        engine(lua).lock().print(
            MessageType::Info,
            "TestService: Doesn't include any assertions",
        );
    } else {
        engine(lua).lock().print(
            MessageType::Info,
            format!("TestService: Run completed, tests: {tests}, warns: {warns}, errors: {errors}"),
        );
    }
    let scripts: Vec<InstanceId> = {
        let eng = engine(lua);
        let e = eng.lock();
        match e.dm.find_service("TestService") {
            Some(ts) => e.dm.children_of(ts).to_vec(),
            None => Vec::new(),
        }
    };
    for s in scripts {
        crate::scripts::stop_script(lua, s);
    }
}

fn done(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    finish(lua);
    none()
}

fn get_command_names(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let t = lua.create_table()?;
    for (i, name) in super::verbs::COMMANDS.iter().enumerate() {
        t.raw_set(i + 1, *name)?;
    }
    ret(lua, t)
}

fn do_command(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = arg_string(args.first()).unwrap_or_default();
    if super::verbs::run(lua, &name) {
        crate::runtime::pump(lua);
        return none();
    }
    Err(rt(format!("Unknown command {name}")))
}

fn command_enabled(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = arg_string(args.first()).unwrap_or_default();
    match super::verbs::enabled(lua, &name) {
        Some(v) => ret(lua, v),
        None => Err(rt(format!("Unknown command {name}"))),
    }
}

fn command_checked(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = arg_string(args.first()).unwrap_or_default();
    match super::verbs::enabled(lua, &name) {
        Some(_) => ret(lua, false),
        None => Err(rt(format!("Unknown command {name}"))),
    }
}

fn run(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    start(lua, crate::runtime::current_identity(lua))?;
    none()
}

pub fn on_timeout(lua: &Lua) {
    let (running, deadline, now, timeout) = {
        let eng = engine(lua);
        let e = eng.lock();
        let timeout =
            e.dm.find_service("TestService")
                .and_then(|ts| e.dm.get(ts, "Timeout").ok())
                .and_then(|v| v.as_f64())
                .unwrap_or(10.0);
        (e.test.running, e.test.deadline, e.game_time, timeout)
    };
    if running && now >= deadline {
        bump(lua, true, false);
        let text = format!(
            "Tests failed to complete in {} seconds",
            crate::format::format_g(timeout, 6)
        );
        engine(lua)
            .lock()
            .print(MessageType::Error, format!("TestService: {text}"));
        record_failure(lua, text);
        finish(lua);
    }
}

pub fn on_script_finished(lua: &Lua, run_count: u32, error: Option<String>) {
    let current = engine(lua).lock().test.run_count;
    if current != run_count {
        return;
    }
    if let Some(msg) = error {
        if let Some(ts) = test_service(lua) {
            let args = [Value::String(match lua.create_string(&msg) {
                Ok(s) => s,
                Err(_) => return,
            })];
            let _ = fail(lua, ts, &args);
        }
        return;
    }
    let remaining = {
        let eng = engine(lua);
        let mut e = eng.lock();
        e.test.scripts = e.test.scripts.saturating_sub(1);
        e.test.scripts
    };
    if remaining == 0 {
        finish(lua);
    }
}

pub fn start(lua: &Lua, identity: Identity) -> Result<()> {
    let Some(ts) = test_service(lua) else {
        return Err(rt("TestService not found"));
    };
    let (scripts, run_count, start_runtime) = {
        let eng = engine(lua);
        let mut e = eng.lock();
        if e.test.running {
            return Err(rt("Run still in progress"));
        }
        let timeout =
            e.dm.get(ts, "Timeout")
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(10.0);
        let now = e.game_time;
        let run_count = e.test.run_count + 1;
        e.test = TestState {
            running: true,
            run_count,
            deadline: now + timeout,
            ..Default::default()
        };
        let scripts: Vec<InstanceId> =
            e.dm.children_of(ts)
                .iter()
                .copied()
                .filter(|&c| e.dm.is_a(c, "Script"))
                .collect();
        e.test.scripts = scripts.len() as u32;
        let auto_runs =
            e.dm.get(ts, "AutoRuns")
                .ok()
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
        let start_runtime = auto_runs && e.stopped;
        if auto_runs {
            e.running = true;
            e.stopped = false;
        }
        (scripts, run_count, start_runtime)
    };
    if start_runtime {
        crate::scripts::scan_all(lua);
    }
    for s in scripts {
        let (source, name) = {
            let eng = engine(lua);
            let e = eng.lock();
            let src =
                e.dm.get(s, "Source")
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_default();
            (src, e.dm.full_name(s))
        };
        let filtered = match filter_script(&source) {
            Ok(f) => f,
            Err((line, msg)) => {
                on_script_finished(lua, run_count, Some(format!("{name}:{line}: {msg}")));
                continue;
            }
        };
        with_state(lua, |st| {
            st.running_scripts.insert(s, true);
            st.test_scripts.insert(s);
        });
        let env = new_environment(lua, Some(s))?;
        let func = match lua
            .load(&filtered)
            .set_name(format!("={name}"))
            .set_environment(env)
            .into_function()
        {
            Ok(f) => f,
            Err(e) => {
                on_script_finished(lua, run_count, Some(crate::runtime::format_error(&e)));
                continue;
            }
        };
        let thread = lua.create_thread(func)?;
        with_state(lua, |st| {
            st.threads.insert(
                thread_key(&thread),
                ThreadInfo {
                    script: Some(s),
                    identity,
                },
            );
            st.script_threads.entry(s).or_default().push(thread.clone());
            st.finish_hooks.insert(thread_key(&thread), run_count);
        });
        resume_thread(lua, &thread, MultiValue::new());
    }
    Ok(())
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
        i += 1;
    }
    i
}

fn closing(c: u8) -> u8 {
    match c {
        b'(' => b')',
        b'[' => b']',
        _ => b'}',
    }
}

fn parse_string(b: &[u8], mut i: usize) -> std::result::Result<usize, String> {
    let close = b[i];
    i += 1;
    while i < b.len() {
        if b[i] == b'\\' {
            i += 1;
        } else if b[i] == close {
            return Ok(i + 1);
        }
        i += 1;
    }
    Err("Missing string closure".into())
}

fn parse_arg(b: &[u8], mut i: usize, close: u8) -> std::result::Result<usize, String> {
    while i < b.len() {
        let c = b[i];
        if c == close || c == b',' {
            return Ok(i);
        }
        if c == b'\'' || c == b'"' {
            i = parse_string(b, i)?;
        } else if c == b'(' || c == b'[' {
            i = parse_bracket(b, i, &mut |_, _| {})?;
        } else {
            i += 1;
        }
    }
    Ok(b.len())
}

pub fn args_in_bracket(text: &str) -> Vec<String> {
    let b = text.as_bytes();
    let Some(start) = b.iter().position(|c| *c == b'(') else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let _ = parse_bracket(b, start, &mut |s, e| out.push(text[s..e].to_string()));
    out
}

fn parse_bracket(
    b: &[u8],
    mut i: usize,
    emit: &mut dyn FnMut(usize, usize),
) -> std::result::Result<usize, String> {
    let close = closing(b[i]);
    i += 1;
    let mut found_comma = true;
    while i < b.len() {
        let c = b[i];
        if c == close {
            return Ok(i + 1);
        }
        if c == b' ' || c == b'\t' {
            i += 1;
            continue;
        }
        if c == b',' {
            found_comma = true;
            i += 1;
            continue;
        }
        if !found_comma {
            return Err("argument not separated by comma".into());
        }
        let end = parse_arg(b, i, close)?;
        let mut last = i;
        for (k, &ch) in b.iter().enumerate().take(end).skip(i) {
            if ch != b' ' && ch != b'\t' {
                last = k + 1;
            }
        }
        if last != i {
            emit(i, last);
        }
        i = end;
        found_comma = false;
    }
    Err("Missing closing bracket".into())
}

const EQUALITY: &[(&str, &str, &str, &str)] = &[
    ("RBX_CHECK_EQUAL", "Check", "==", "!="),
    ("RBX_CHECK_NE", "Check", "~=", "=="),
    ("RBX_CHECK_GE", "Check", ">=", "<"),
    ("RBX_CHECK_LE", "Check", "<=", ">"),
    ("RBX_CHECK_GT", "Check", ">", "<="),
    ("RBX_CHECK_LT", "Check", "<", ">="),
    ("RBX_WARN_EQUAL", "Warn", "==", "!="),
    ("RBX_WARN_NE", "Warn", "!=", "=="),
    ("RBX_WARN_GE", "Warn", ">=", "<"),
    ("RBX_WARN_LE", "Warn", "<=", ">"),
    ("RBX_WARN_GT", "Warn", ">", "<="),
    ("RBX_WARN_LT", "Warn", "<", ">="),
    ("RBX_REQUIRE_EQUAL", "Require", "==", "!="),
    ("RBX_REQUIRE_NE", "Require", "!=", "=="),
    ("RBX_REQUIRE_GE", "Require", ">=", "<"),
    ("RBX_REQUIRE_LE", "Require", "<=", ">"),
    ("RBX_REQUIRE_GT", "Require", ">", "<="),
    ("RBX_REQUIRE_LT", "Require", "<", ">="),
];

enum Kind {
    Simple,
    Throw,
    NoThrow,
    Test,
}

const OTHERS: &[(&str, &str, Kind)] = &[
    ("RBX_WARN_MESSAGE", "Warn", Kind::Simple),
    ("RBX_CHECK_MESSAGE", "Check", Kind::Simple),
    ("RBX_REQUIRE_MESSAGE", "Require", Kind::Simple),
    ("RBX_WARN_THROW", "Warn", Kind::Throw),
    ("RBX_CHECK_THROW", "Check", Kind::Throw),
    ("RBX_REQUIRE_THROW", "Require", Kind::Throw),
    ("RBX_WARN_NO_THROW", "Warn", Kind::NoThrow),
    ("RBX_CHECK_NO_THROW", "Check", Kind::NoThrow),
    ("RBX_REQUIRE_NO_THROW", "Require", Kind::NoThrow),
    ("RBX_WARN", "Warn", Kind::Test),
    ("RBX_CHECK", "Check", Kind::Test),
    ("RBX_REQUIRE", "Require", Kind::Test),
    ("RBX_ERROR", "Error", Kind::Simple),
    ("RBX_FAIL", "Fail", Kind::Simple),
    ("RBX_MESSAGE", "Message", Kind::Simple),
    ("RBX_CHECKPOINT", "Checkpoint", Kind::Simple),
];

fn process_line(n: usize, line: &str) -> std::result::Result<String, String> {
    let b = line.as_bytes();
    for (macro_name, f, eq, ne) in EQUALITY {
        let Some(i) = line.find(macro_name) else {
            continue;
        };
        let start = skip_ws(b, i + macro_name.len());
        if b.get(start) != Some(&b'(') {
            return Err("Expected '('".into());
        }
        let mut args: Vec<(usize, usize)> = Vec::new();
        let stop = parse_bracket(b, start, &mut |s, e| args.push((s, e)))?;
        if args.len() != 2 {
            return Err(format!("Expected 2 arguments but got {}", args.len()));
        }
        let a = &line[args[0].0..args[0].1];
        let bb = &line[args[1].0..args[1].1];
        let mut out = String::new();
        out.push_str(&line[..i]);
        out.push_str("do ");
        out.push_str(&format!("local aZZZZ = {a} local bZZZZ = {bb} "));
        out.push_str(&format!("if aZZZZ {eq} bZZZZ then "));
        out.push_str(&format!(
            "Game:GetService('TestService'):{f}(true, [==[{a} {eq} {bb}]==], script, {n}) "
        ));
        out.push_str("else ");
        out.push_str(&format!(
            "Game:GetService('TestService'):{f}(false, [==[test {a} {eq} {bb} failed []==] .. tostring(aZZZZ) .. ' {ne} ' .. tostring(bZZZZ) .. ']', script, {n}) "
        ));
        out.push_str("end ");
        out.push_str("end");
        out.push_str(&line[stop..]);
        return Ok(out);
    }
    for (macro_name, f, kind) in OTHERS {
        let Some(i) = line.find(macro_name) else {
            continue;
        };
        let start = skip_ws(b, i + macro_name.len());
        if b.get(start) != Some(&b'(') {
            return Err("Expected '('".into());
        }
        let stop = parse_bracket(b, start, &mut |_, _| {})?;
        let inner = &line[start + 1..stop - 1];
        let mut out = String::new();
        out.push_str(&line[..i]);
        match kind {
            Kind::Simple => {
                out.push_str(&format!(
                    "Game:GetService('TestService'):{f}({inner}, script, {n})"
                ));
            }
            Kind::Test => {
                out.push_str(&format!(
                    "Game:GetService('TestService'):{f}({inner}, [==[test failed: {inner}]==], script, {n})"
                ));
            }
            Kind::Throw => {
                out.push_str(&format!(
                    "Game:GetService('TestService'):{f}(ypcall(function() {inner} end) == false, 'Exception expected in ' .. [==[{inner}]==], script, {n})"
                ));
            }
            Kind::NoThrow => {
                out.push_str(&format!(
                    "Game:GetService('TestService'):{f}(ypcall(function() {inner} end) == true, 'Exception thrown by ' .. [==[{inner}]==], script, {n})"
                ));
            }
        }
        out.push_str(&line[stop..]);
        return Ok(out);
    }
    Ok(line.to_string())
}

pub fn filter_script(source: &str) -> std::result::Result<String, (usize, String)> {
    let mut out = String::with_capacity(source.len() + 256);
    for (i, line) in source.split('\n').enumerate() {
        if line.len() > 1000 {
            return Err((i + 1, "Line too long".into()));
        }
        let processed = process_line(i + 1, line).map_err(|e| (i + 1, e))?;
        out.push_str(&processed);
        out.push('\n');
    }
    Ok(out)
}

pub fn register(r: &mut Registry) {
    r.add("TestService", &["Check"], check);
    r.add("TestService", &["Warn"], warn);
    r.add("TestService", &["Require"], require);
    r.add("TestService", &["Error"], error);
    r.add("TestService", &["Fail"], fail);
    r.add("TestService", &["Message"], message);
    r.add("TestService", &["Checkpoint"], checkpoint);
    r.add("TestService", &["Done"], done);
    r.add("TestService", &["Run"], run);
    r.add("TestService", &["GetCommandNames"], get_command_names);
    r.add("TestService", &["DoCommand"], do_command);
    r.add("TestService", &["IsCommandEnabled"], command_enabled);
    r.add("TestService", &["IsCommandChecked"], command_checked);
    r.add("FunctionalTest", &["Pass", "Passed"], functional_pass);
    r.add("FunctionalTest", &["Error", "Failed"], functional_error);
    r.add("FunctionalTest", &["Warn"], functional_warn);
}

fn functional_pass(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let text = arg_string(args.first()).unwrap_or_default();
    bump(lua, false, false);
    output(lua, MessageType::Info, None, 0, &format!("Passed {text}"));
    none()
}

fn functional_error(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let text = arg_string(args.first()).unwrap_or_default();
    bump(lua, true, false);
    output(lua, MessageType::Error, None, 0, &text);
    record_failure(lua, text);
    none()
}

fn functional_warn(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let text = arg_string(args.first()).unwrap_or_default();
    bump(lua, false, true);
    output(lua, MessageType::Warning, None, 0, &text);
    none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macros_expand() {
        let src = "RBX_CHECK(x == 1)\nRBX_CHECK_EQUAL(a, b)\nRBX_CHECK_MESSAGE(true, 'ok')";
        let out = filter_script(src).expect("filter");
        assert!(out.contains(
            "Game:GetService('TestService'):Check(x == 1, [==[test failed: x == 1]==], script, 1)"
        ));
        assert!(out.contains("local aZZZZ = a local bZZZZ = b"));
        assert!(out.contains("Check(true, 'ok', script, 3)"));
    }
}
