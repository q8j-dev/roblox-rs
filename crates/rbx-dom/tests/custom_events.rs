use rbx_dom::{DataModel, InstanceId, LoadOptions, SaveOptions, Variant};

struct Wired {
    dm: DataModel,
    part: InstanceId,
    evt: InstanceId,
    receiver: InstanceId,
}

fn wired() -> Wired {
    let mut dm = DataModel::new();
    let ws = dm.get_service("Workspace").expect("workspace");
    let part = dm.create("Part").expect("part");
    dm.set(part, "Anchored", Variant::Bool(true))
        .expect("anchor");
    dm.set_parent(part, Some(ws)).expect("parent");
    let evt = dm.create("CustomEvent").expect("event");
    let receiver = dm.create("CustomEventReceiver").expect("receiver");
    dm.set_parent(evt, Some(part)).expect("parent");
    dm.set_parent(receiver, Some(part)).expect("parent");
    dm.set(receiver, "Source", Variant::Ref(Some(evt)))
        .expect("source");
    dm.drain_signals();
    Wired {
        dm,
        part,
        evt,
        receiver,
    }
}

fn copy_paste(dm: &mut DataModel, items: &[InstanceId], target: InstanceId) -> Vec<InstanceId> {
    let tree = dm.save_tree(items, SaveOptions::default());
    let (tops, _) = dm
        .load_tree(&tree, Some(target), LoadOptions::default())
        .expect("paste");
    tops
}

fn value_changes(dm: &mut DataModel, receiver: InstanceId) -> usize {
    dm.drain_signals()
        .iter()
        .filter(|(id, e, _, _)| *id == receiver && *e == "SourceValueChanged")
        .count()
}

#[test]
fn studio_copy_event() {
    let mut w = wired();
    let pasted = copy_paste(&mut w.dm, &[w.evt], w.part);
    assert_eq!(pasted.len(), 1);
    let pe = pasted[0];
    assert!(w.dm.custom_attached_receivers(pe).is_empty());
    assert_eq!(w.dm.get_ref(w.receiver, "Source"), Some(w.evt));
    assert_eq!(w.dm.custom_attached_receivers(w.evt).len(), 1);
    w.dm.custom_event_set_value(pe, 1.0);
    assert_eq!(value_changes(&mut w.dm, w.receiver), 0);
    w.dm.custom_event_set_value(w.evt, 1.0);
    assert_eq!(value_changes(&mut w.dm, w.receiver), 1);
}

#[test]
fn studio_copy_receiver() {
    let mut w = wired();
    let pasted = copy_paste(&mut w.dm, &[w.receiver], w.part);
    assert_eq!(pasted.len(), 1);
    let pr = pasted[0];
    assert_eq!(w.dm.get_ref(w.receiver, "Source"), Some(w.evt));
    assert_eq!(w.dm.custom_attached_receivers(w.evt).len(), 1);
    w.dm.custom_event_set_value(w.evt, 1.0);
    assert_eq!(w.dm.custom_receiver_value(w.receiver), 1.0);
    assert_eq!(w.dm.custom_receiver_value(pr), 0.0);
}

#[test]
fn studio_copy_event_and_receiver() {
    let mut w = wired();
    let pasted = copy_paste(&mut w.dm, &[w.evt, w.receiver], w.part);
    assert_eq!(pasted.len(), 2);
    let (pe, pr) = (pasted[0], pasted[1]);
    assert_ne!(pe, w.evt);
    assert_ne!(pr, w.receiver);
    assert_eq!(w.dm.custom_attached_receivers(pe), vec![pr]);
    assert_eq!(w.dm.get_ref(pr, "Source"), Some(pe));
    assert_eq!(w.dm.custom_receiver_value(pr), 0.0);
    w.dm.custom_event_set_value(pe, 1.0);
    assert_eq!(w.dm.custom_receiver_value(pr), 1.0);
    assert_eq!(value_changes(&mut w.dm, w.receiver), 0);
    w.dm.custom_event_set_value(w.evt, 1.0);
    assert_eq!(value_changes(&mut w.dm, w.receiver), 1);
}

#[test]
fn sender_remembers_last_state() {
    let mut w = wired();
    w.dm.custom_event_set_value(w.evt, 0.5);
    let pasted = copy_paste(&mut w.dm, &[w.evt], w.part);
    let pe = pasted[0];
    assert_eq!(w.dm.custom_event_value(pe), 0.5);
    let r = w.dm.create("CustomEventReceiver").expect("receiver");
    w.dm.drain_signals();
    w.dm.set(r, "Source", Variant::Ref(Some(pe)))
        .expect("source");
    assert_eq!(value_changes(&mut w.dm, r), 1);
    assert_eq!(w.dm.custom_receiver_value(r), 0.5);
}
