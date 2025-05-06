//! Rust Excel File manipulation library

use quick_xml::events::Event;
use quick_xml::reader::Reader;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use zip::ZipArchive;

pub struct XlsxWorkbook {
    pub archive: ZipArchive<File>,
    pub sheets: HashMap<String, String>,
    pub defined_names: HashMap<String, String>,
    pub lang: String,
}

impl XlsxWorkbook {
    pub fn open(xls_path: &str) -> Result<Self, std::io::Error> {
        let f = File::open(xls_path)?;
        let mut archive = ZipArchive::new(f)?;
        let mut buf_workbook = String::new();
        (archive.by_name("xl/workbook.xml")?).read_to_string(&mut buf_workbook)?;
        let mut reader = Reader::from_str(&buf_workbook);
        let mut sheets: HashMap<String, String> = HashMap::new();
        // name: Used for sheet_name in sheet and name in definedName
        let mut name = String::new();
        // object_ref: Used for sheet_id in sheet and value text reference in definedName
        let mut object_ref = String::new();
        let mut defined_names: HashMap<String, String> = HashMap::new();
        loop {
            match reader.read_event() {
                Err(e) => panic!("Error at position {}: {:?}", reader.error_position(), e),
                Ok(Event::Eof) => break,
                Ok(Event::Empty(e)) => {
                    if e.name().as_ref() == b"sheet" {
                        for attr in e.attributes() {
                            match attr {
                                Ok(attr) => match attr.key.as_ref() {
                                    b"sheetId" => {
                                        object_ref = attr.unescape_value().unwrap().into();
                                    }
                                    b"name" => {
                                        name = attr.unescape_value().unwrap().into();
                                    }
                                    _ => (),
                                },
                                Err(e) => {
                                    panic!("Error at position {}: {:?}", reader.error_position(), e)
                                }
                            }
                        }
                        sheets.insert(object_ref.clone(), name.clone());
                    }
                }
                Ok(Event::Start(e)) => {
                    if e.name().as_ref() == b"definedName" {
                        for attr in e.attributes() {
                            match attr {
                                Ok(attr) => match attr.key.as_ref() {
                                    b"name" => {
                                        name = attr.unescape_value().unwrap().into();
                                    }
                                    _ => (),
                                },
                                Err(e) => {
                                    panic!("Error at position {}: {:?}", reader.error_position(), e)
                                }
                            }
                        }
                        match reader.read_event() {
                            Ok(Event::Text(text)) => {
                                object_ref = text.unescape().unwrap().into();
                            }
                            Err(e) => {
                                panic!("Error at position {}: {:?}", reader.error_position(), e)
                            }
                            _ => (),
                        }
                        defined_names.insert(name.clone(), object_ref.clone());
                    }
                }
                _ => (),
            }
        }
        Ok(XlsxWorkbook {
            archive,
            sheets,
            defined_names,
            lang: "".to_string(), // Not implemented yet
        })
    }
}
