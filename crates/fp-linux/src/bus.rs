//! Small blocking D-Bus helpers over `zbus`.

use crate::logic::{self, Val, dbus};
use zbus::blocking::Connection;
use zbus::zvariant::{OwnedValue, Value};

const BUS: &str = "org.freedesktop.DBus";
const BUS_PATH: &str = "/org/freedesktop/DBus";

/// `name` has an owner on the bus behind `conn`.
pub fn name_has_owner(conn: &Connection, name: &str) -> bool {
    conn.call_method(Some(BUS), BUS_PATH, Some(BUS), "NameHasOwner", &(name,))
        .ok()
        .and_then(|m| m.body().deserialize::<bool>().ok())
        .unwrap_or(false)
}

/// `name` can be started by the bus.
pub fn name_activatable(conn: &Connection, name: &str) -> bool {
    conn.call_method(Some(BUS), BUS_PATH, Some(BUS), "ListActivatableNames", &())
        .ok()
        .and_then(|m| m.body().deserialize::<Vec<String>>().ok())
        .is_some_and(|v| v.iter().any(|n| n == name))
}

/// Registers match rules; failures are logged and skipped.
pub fn add_rules(conn: &Connection, rules: &[String]) {
    for r in rules {
        if let Err(e) = conn.call_method(Some(BUS), BUS_PATH, Some(BUS), "AddMatch", &(r.as_str(),)) {
            log::warn!("AddMatch {r}: {e}");
        }
    }
}

/// A D-Bus value as one of the types the services use; variants are opened.
pub fn to_val(v: &Value<'_>) -> Option<Val> {
    match v {
        Value::Bool(b) => Some(Val::Bool(*b)),
        Value::U32(n) => Some(Val::U32(*n)),
        Value::F64(f) => Some(Val::F64(*f)),
        Value::Str(s) => Some(Val::Str(s.as_str().to_string())),
        Value::Value(inner) => to_val(inner),
        _ => None,
    }
}

/// `org.freedesktop.DBus.Properties.Get`.
pub fn get_prop(conn: &Connection, dest: &str, path: &str, iface: &str, prop: &str) -> Option<Val> {
    let reply = conn.call_method(Some(dest), path, Some(dbus::PROPS), "Get", &(iface, prop)).ok()?;
    let v: OwnedValue = reply.body().deserialize().ok()?;
    to_val(&v)
}

/// Portal `Settings.Read`.
pub fn portal_read(conn: &Connection, ns: &str, key: &str) -> Option<Val> {
    let reply = conn
        .call_method(Some(dbus::PORTAL), dbus::PORTAL_PATH, Some(dbus::PORTAL_SETTINGS), "Read", &(ns, key))
        .ok()?;
    let v: OwnedValue = reply.body().deserialize().ok()?;
    to_val(&v)
}

/// Our logind session object path.
pub fn session_path(conn: &Connection) -> Option<String> {
    let by_pid = conn
        .call_method(
            Some(dbus::LOGIN),
            dbus::LOGIN_PATH,
            Some(dbus::LOGIN_MANAGER),
            "GetSessionByPID",
            &(std::process::id(),),
        )
        .ok();
    let reply = by_pid.or_else(|| {
        let id = std::env::var("XDG_SESSION_ID").ok()?;
        conn.call_method(Some(dbus::LOGIN), dbus::LOGIN_PATH, Some(dbus::LOGIN_MANAGER), "GetSession", &(id,)).ok()
    })?;
    let p: zbus::zvariant::OwnedObjectPath = reply.body().deserialize().ok()?;
    logic::valid_session_path(p.as_str()).then(|| p.as_str().to_string())
}

/// Decodes a bus message into a [`logic::Signal`] when it is one we use.
pub fn decode(msg: &zbus::Message) -> Option<logic::Signal> {
    use logic::Signal;
    let header = msg.header();
    if header.message_type() != zbus::message::Type::Signal {
        return None;
    }
    let iface = header.interface()?.as_str().to_string();
    let member = header.member()?.as_str().to_string();
    let body = msg.body();
    match (iface.as_str(), member.as_str()) {
        (dbus::LOGIN_MANAGER, "PrepareForSleep") => Some(Signal::PrepareForSleep(body.deserialize::<bool>().ok()?)),
        (dbus::LOGIN_SESSION, "Lock") => Some(Signal::Lock),
        (dbus::LOGIN_SESSION, "Unlock") => Some(Signal::Unlock),
        (dbus::PROPS, "PropertiesChanged") => {
            let (i, changed, _): (String, std::collections::HashMap<String, OwnedValue>, Vec<String>) =
                body.deserialize().ok()?;
            let changed = changed.iter().filter_map(|(k, v)| Some((k.clone(), to_val(v)?))).collect();
            Some(Signal::Props { iface: i, changed })
        }
        (dbus::PORTAL_SETTINGS, "SettingChanged") => {
            let (ns, key, v): (String, String, OwnedValue) = body.deserialize().ok()?;
            Some(Signal::Portal { ns, key, value: to_val(&v)? })
        }
        (i, "ActiveChanged") if i.to_ascii_lowercase().ends_with("screensaver") => {
            Some(Signal::ScreenSaver(body.deserialize::<bool>().ok()?))
        }
        _ => None,
    }
}
