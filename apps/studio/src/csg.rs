use rbx_dom::InstanceId;
use rbx_studio_ui::Document;

fn run<T>(
    doc: &mut Document,
    op: impl FnOnce(&mut rbx_dom::DataModel, &[InstanceId]) -> Result<T, String>,
) -> Result<T, String> {
    let selection = doc.selection();
    doc.checkpoint();
    let out = {
        let mut e = doc.runtime.engine.lock();
        op(&mut e.dm, &selection)
    };
    if out.is_ok() {
        doc.dirty = true;
    }
    out
}

pub fn union(doc: &mut Document) -> Result<(), String> {
    let op = run(doc, rbx_script::csg::union)?;
    doc.select(vec![op]);
    Ok(())
}

pub fn negate(doc: &mut Document) -> Result<(), String> {
    let ops = run(doc, rbx_script::csg::negate)?;
    doc.select(ops);
    Ok(())
}

pub fn separate(doc: &mut Document) -> Result<(), String> {
    let parts = run(doc, rbx_script::csg::separate)?;
    doc.select(parts);
    Ok(())
}
