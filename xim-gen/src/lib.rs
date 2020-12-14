use serde::Deserialize;
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::Path;

#[derive(Deserialize)]
#[cfg_attr(debug_assertions, derive(Debug, Eq, PartialEq))]
struct EnumFormat {
    repr: String,
    variants: HashMap<String, usize>,
}

impl EnumFormat {
    pub fn write(&self, name: &str, out: &mut impl Write) -> io::Result<()> {
        writeln!(out, "#[derive(Clone, Copy, Debug)]")?;
        writeln!(out, "#[repr({})]", self.repr)?;
        writeln!(out, "pub enum {} {{", name)?;

        for (name, variant) in self.variants.iter() {
            writeln!(out, "{} = {},", name, variant)?;
        }

        writeln!(out, "}}")?;

        writeln!(out, "impl<'b> XimFormat<'b> for {} {{", name)?;

        writeln!(
            out,
            "fn read(reader: &mut Reader<'b>) -> Result<Self, ReadError> {{
    let repr = {repr}::read(reader)?;
    match repr {{",
            repr = self.repr
        )?;

        for (name, variants) in self.variants.iter() {
            writeln!(out, "{v} => Ok(Self::{n}),", v = variants, n = name)?;
        }

        writeln!(
            out,
            "_ => Err(reader.invalid_data(\"{n}\", repr)),",
            n = name
        )?;

        writeln!(out, "}}}}")?;

        writeln!(
            out,
            "fn write(&self, writer: &mut Writer) {{
            (*self as {repr}).write(writer);
            }}",
            repr = self.repr
        )?;

        writeln!(
            out,
            "fn size(&self) -> usize {{ std::mem::size_of::<{}>() }}",
            self.repr
        )?;

        // impl
        writeln!(out, "}}")?;

        Ok(())
    }
}

#[derive(Deserialize)]
#[cfg_attr(debug_assertions, derive(Debug, Eq, PartialEq))]
struct RequestFormat {
    major_opcode: u8,
    minor_opcode: Option<u8>,
    body: HashMap<String, String>,
}

#[derive(Deserialize)]
#[cfg_attr(debug_assertions, derive(Debug, Eq, PartialEq))]
struct XimFormat {
    #[serde(rename = "Enums")]
    enums: HashMap<String, EnumFormat>,
    #[serde(rename = "Requests")]
    requests: HashMap<String, RequestFormat>,
}

impl XimFormat {
    pub fn write(&self, out: &mut impl Write) -> io::Result<()> {
        for (name, em) in self.enums.iter() {
            em.write(name, out)?;
        }

        writeln!(out, "#[derive(Debug, Clone, Eq, PartialEq)]")?;
        writeln!(out, "pub enum Request<'b> {{")?;

        for (name, req) in self.requests.iter() {
            writeln!(out, "{} {{", name)?;
            for (field, ty) in req.body.iter() {
                writeln!(out, "{}: {},", field, ty)?;
            }
            writeln!(out, "}},")?;
        }

        writeln!(out, "}}")?;

        writeln!(out, "impl<'b> XimFormat<'b> for Request<'b> {{")?;

        writeln!(
            out,
            "fn read(reader: &mut Reader<'b>) -> Result<Self, ReadError> {{"
        )?;

        writeln!(
            out,
            "let major_opcode = reader.u16()?; let minor_opcode = reader.u16()?;"
        )?;

        writeln!(out, "match (major_opcode, minor_opcode) {{")?;

        for (name, req) in self.requests.iter() {
            write!(out, "({}, ", req.major_opcode)?;

            if let Some(minor) = req.minor_opcode {
                write!(out, "{}", minor)?;
            } else {
                write!(out, "_")?;
            }

            writeln!(out, ") => Ok(Request::{} {{", name)?;
            for (field, _ty) in req.body.iter() {
                writeln!(out, "{}: XimFormat::read(reader)?,", field)?;
            }
            writeln!(out, "}}),")?;
        }

        writeln!(out, "_ => Err(reader.invalid_data(\"Opcode\", format!(\"({{}}, {{}})\", major_opcode, minor_opcode))),")?;

        // match
        writeln!(out, "}}")?;

        // fn read
        writeln!(out, "}}")?;

        writeln!(out, "fn write(&self, writer: &mut Writer) {{")?;

        writeln!(out, "match self {{")?;

        for (name, req) in self.requests.iter() {
            writeln!(out, "Request::{} {{", name)?;
            for (field, _ty) in req.body.iter() {
                write!(out, "{}, ", field)?;
            }
            writeln!(out, "}} => {{")?;

            writeln!(out, "{}u8.write(writer);", req.major_opcode)?;
            writeln!(out, "{}u8.write(writer);", req.minor_opcode.unwrap_or(0))?;
            writeln!(out, "(((self.size() - 4) / 4) as u16).write(writer);")?;

            for (field, _ty) in req.body.iter() {
                writeln!(out, "{}.write(writer);", field)?;
            }

            writeln!(out, "}}")?;
        }

        // match
        writeln!(out, "}}")?;

        // fn write
        writeln!(out, "}}")?;

        writeln!(out, "fn size(&self) -> usize {{")?;
        writeln!(out, "let mut content_size = 0;")?;

        writeln!(out, "match self {{")?;

        for (name, req) in self.requests.iter() {
            writeln!(out, "Request::{} {{", name)?;
            for (field, _ty) in req.body.iter() {
                write!(out, "{}, ", field)?;
            }
            writeln!(out, "}} => {{")?;

            for (field, _ty) in req.body.iter() {
                writeln!(out, "content_size += {}.size();", field)?;
            }

            writeln!(out, "}}")?;
        }

        // match
        writeln!(out, "}}")?;
        writeln!(out, "content_size + 4")?;

        // fn size
        writeln!(out, "}}")?;

        // impl
        writeln!(out, "}}")?;

        Ok(())
    }
}

pub fn write_format(
    format_str: &str,
    out_path: impl AsRef<Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let format: XimFormat = serde_yaml::from_str(format_str)?;

    let mut file = std::fs::File::create(out_path.as_ref())?;

    file.write_all(include_bytes!("../res/snippet.rs"))?;
    format.write(&mut file)?;
    file.flush()?;

    let rustfmt = std::process::Command::new("rustfmt")
        .arg(std::fs::canonicalize(out_path.as_ref())?)
        .spawn()
        .expect("call rustfmt")
        .wait()
        .unwrap();

    assert!(rustfmt.success());

    Ok(())
}
