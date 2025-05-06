use quick_xml::events::Event;
use quick_xml::events::attributes::Attributes;
use quick_xml::reader::Reader;
use std::collections::HashMap;
use std::env;
use std::io::Read;
use xlslang_file::XlsxWorkbook;

fn main() -> std::io::Result<()> {
    let mut xls_path = String::new();
    if let Some(arg1) = env::args().nth(1) {
        xls_path = arg1;
    } else {
        println!("Parámetro de archivo");
        return Ok(());
    }
    let mut workbook = XlsxWorkbook::open(&xls_path)?;
    let mut buf_workbook = String::new();
    let mut attrs: Attributes;
    let mut cell = String::new();
    let mut is_error = false;
    let mut has_errors = false;
    let mut cells_with_errors_insheet: Vec<String> = Vec::new();
    let mut cells_with_errors: HashMap<String, Vec<String>> = HashMap::new();

    for (sheet_id, sheet_name) in &workbook.sheets {
        (workbook
            .archive
            .by_name(&format!("xl/worksheets/sheet{sheet_id}.xml"))?)
        .read_to_string(&mut buf_workbook)?;
        let mut reader = Reader::from_str(&buf_workbook);
        loop {
            match reader.read_event() {
                Err(e) => panic!("Error at position {}: {:?}", reader.error_position(), e),
                Ok(Event::Eof) => break,
                Ok(Event::Start(e)) => {
                    if e.name().as_ref() == b"c" {
                        attrs = e.attributes();
                        for attr in attrs {
                            match attr {
                                Ok(attr) => match attr.key.as_ref() {
                                    b"r" => {
                                        cell = attr.unescape_value().unwrap().into();
                                    }
                                    b"t" => {
                                        is_error = attr.unescape_value().unwrap() == "e";
                                    }
                                    _ => (),
                                },
                                Err(e) => {
                                    panic!("Error at position {}: {:?}", reader.error_position(), e)
                                }
                            }
                        }
                        if is_error {
                            cells_with_errors_insheet.push(cell.clone());
                            has_errors = true;
                            is_error = false;
                        }
                    }
                }
                _ => (),
            }
        }

        if has_errors {
            has_errors = false;
            cells_with_errors.insert(sheet_name.clone(), cells_with_errors_insheet.clone());
            cells_with_errors_insheet.clear();
        }
        buf_workbook.clear();
    }

    println!("Errores totales {:#?}", cells_with_errors);
    Ok(())
}
