//! Print where the engine and the LibreOffice-computed probe workbook differ.
use serde_json::Value as Json;
use xlslang::addr::CellRef;
use xlslang::{BindingSpec, Overrides, Spec, Value, compile, loader};

fn main() {
    let file = std::env::args()
        .nth(1)
        .unwrap_or("tests/assets/probe.ods".into());
    let m = loader::load(std::path::Path::new(&file)).unwrap();
    let calc = m.sheet_id("Calc").unwrap();
    let n = m
        .cells
        .keys()
        .filter(|a| a.sheet == calc && a.col == 0)
        .count() as u32;
    let spec = Spec {
        outputs: vec![BindingSpec::parse(&format!("Calc!A1:A{n}"))],
        fold: false,
        ..Spec::default()
    };
    let (p, _) = compile(&m, &spec).unwrap();
    let out = p.execute(&Json::Null, &Overrides::default()).unwrap();
    let col = out[&p.outputs[0].name].as_array().unwrap();
    for r in 0..n {
        let label = match &m.cells[&CellRef::new(calc, r, 1)].value {
            Value::Text(t) => t.to_string(),
            _ => String::new(),
        };
        let lo = format!("{:?}", m.cells[&CellRef::new(calc, r, 0)].value);
        let ours = col[r as usize][0].to_string();
        println!("{label:<48} LO={lo:<28} ours={ours}");
    }
}
