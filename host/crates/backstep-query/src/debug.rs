//! The firmware's debug information: source lines and function names for
//! addresses, and addresses for variables (`g_rx.limit`, `buf[3]`), read
//! from the ELF's DWARF sections, with the symbol table as a fallback.

use std::fmt;

use backstep_emu::elf::Elf;
use gimli::{AttributeValue, EndianSlice, LittleEndian, Reader as _, UnitOffset};
use serde::Serialize;

type R<'a> = EndianSlice<'a, LittleEndian>;
/// A line-table row: (start, end, file index, line).
type LineRow = (u32, u32, usize, u32);
/// A type reference: (unit index, offset in the unit).
type TypeRef = Option<(usize, UnitOffset)>;

/// Where an address is in the source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Location {
    /// The address.
    pub address: u32,
    /// The innermost function (the inlined one, if inlined).
    pub function: Option<String>,
    /// Source file.
    pub file: Option<String>,
    /// Source line.
    pub line: Option<u32>,
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#010x}", self.address)?;
        if let Some(func) = &self.function {
            write!(f, " in {func}")?;
        }
        if let (Some(file), Some(line)) = (&self.file, self.line) {
            let short = file.rsplit('/').next().unwrap_or(file);
            write!(f, " at {short}:{line}")?;
        }
        Ok(())
    }
}

/// A variable or part of one, in memory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Variable {
    /// As asked for.
    pub expression: String,
    /// First byte.
    pub address: u32,
    /// Bytes.
    pub size: u32,
    /// Its type, as far as known.
    #[serde(rename = "type")]
    pub type_name: String,
}

/// Why a variable expression could not be resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookupError(pub String);

impl fmt::Display for LookupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LookupError {}

/// The debug information of a firmware ELF.
pub struct DebugInfo<'a> {
    dwarf: gimli::Dwarf<R<'a>>,
    frames: Option<addr2line::Context<R<'a>>>,
    /// Line table rows (address, file, line) of every sequence, sorted;
    /// see [`line_rows`].
    lines: Vec<LineRow>,
    files: Vec<String>,
    functions: Vec<(String, u32, u32)>,
    symbols: &'a Elf,
}

/// Every line-table sequence as rows (start, end, file index, line),
/// leaving out sequences that start at address 0: those are functions the
/// linker discarded (`--gc-sections`), whose addresses it set to 0, and
/// they would otherwise cover the real code at low addresses (where the
/// vector table is, no function starts at 0).
fn line_rows(dwarf: &gimli::Dwarf<R<'_>>) -> (Vec<LineRow>, Vec<String>) {
    let mut rows_out = Vec::new();
    let mut files: Vec<String> = Vec::new();
    let mut units = dwarf.units();
    while let Ok(Some(header)) = units.next() {
        let Ok(unit) = dwarf.unit(header) else {
            continue;
        };
        let Some(program) = unit.line_program.clone() else {
            continue;
        };
        let mut rows = program.rows();
        let mut sequence: Vec<(u32, usize, u32)> = Vec::new();
        while let Ok(Some((header, row))) = rows.next_row() {
            if row.end_sequence() {
                let end = row.address() as u32;
                if sequence.first().is_some_and(|s| s.0 != 0) {
                    for (i, &(a, f, l)) in sequence.iter().enumerate() {
                        let next = sequence.get(i + 1).map_or(end, |n| n.0);
                        if next > a {
                            rows_out.push((a, next, f, l));
                        }
                    }
                }
                sequence.clear();
                continue;
            }
            let name = header
                .file(row.file_index())
                .and_then(|f| dwarf.attr_string(&unit, f.path_name()).ok())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let index = files.iter().position(|f| *f == name).unwrap_or_else(|| {
                files.push(name);
                files.len() - 1
            });
            let line = row.line().map_or(0, |l| l.get() as u32);
            sequence.push((row.address() as u32, index, line));
        }
    }
    rows_out.sort_by_key(|r| r.0);
    (rows_out, files)
}

fn load<'a>(elf: &'a Elf) -> Result<gimli::Dwarf<R<'a>>, gimli::Error> {
    gimli::Dwarf::load(|id| -> Result<R<'a>, gimli::Error> {
        Ok(EndianSlice::new(
            elf.section(id.name()).unwrap_or(&[]),
            LittleEndian,
        ))
    })
}

/// A resolved type: its name, size, and how to look inside it.
#[derive(Debug, Clone)]
enum Shape {
    Scalar,
    Struct(Vec<(String, u32, TypeRef)>),
    Array {
        element: TypeRef,
        count: Option<u32>,
    },
}

#[derive(Debug, Clone)]
struct Type {
    name: String,
    size: u32,
    shape: Shape,
}

impl<'a> DebugInfo<'a> {
    /// Read the debug information (absent sections make an empty one).
    #[must_use]
    pub fn new(elf: &'a Elf) -> Self {
        let dwarf = load(elf).unwrap_or_default();
        let frames = load(elf)
            .ok()
            .and_then(|d| addr2line::Context::from_dwarf(d).ok());
        let (lines, files) = line_rows(&dwarf);
        DebugInfo {
            dwarf,
            frames,
            lines,
            files,
            functions: elf.functions(),
            symbols: elf,
        }
    }

    /// The function containing `address` in the symbol table.
    fn symbol_function(&self, address: u32) -> Option<String> {
        let i = self.functions.partition_point(|f| f.1 <= address);
        let (name, start, size) = self.functions.get(i.checked_sub(1)?)?;
        (address < start + (*size).max(2)).then(|| name.clone())
    }

    /// Source location of `address`.
    #[must_use]
    pub fn location(&self, address: u32) -> Location {
        let address = address & !1;
        let mut loc = Location {
            address,
            function: None,
            file: None,
            line: None,
        };
        let i = self.lines.partition_point(|r| r.0 <= address);
        if let Some(&(start, end, file, line)) = i.checked_sub(1).and_then(|i| self.lines.get(i)) {
            if address >= start && address < end && line != 0 {
                loc.file = self.files.get(file).cloned();
                loc.line = Some(line);
            }
        }
        // The function: the symbol table's, or the innermost inlined one
        // when the debug information agrees on the outermost.
        let symbol = self.symbol_function(address);
        loc.function.clone_from(&symbol);
        if let Some(ctx) = &self.frames {
            if let Ok(mut frames) = ctx.find_frames(u64::from(address)).skip_all_loads() {
                let mut names = Vec::new();
                while let Ok(Some(frame)) = frames.next() {
                    if let Some(n) = frame
                        .function
                        .and_then(|f| f.raw_name().ok().map(std::borrow::Cow::into_owned))
                    {
                        names.push(n);
                    }
                }
                if names
                    .last()
                    .is_some_and(|outer| Some(outer) == symbol.as_ref())
                {
                    loc.function = names.first().cloned();
                }
            }
        }
        loc
    }

    /// The address and size of `expression`: a global variable's name,
    /// then any number of `.member` and `[index]`; or `0xADDRESS:SIZE`.
    ///
    /// # Errors
    /// If it names nothing known.
    pub fn variable(&self, expression: &str) -> Result<Variable, LookupError> {
        let expr = expression.trim();
        if let Some(raw) = expr.strip_prefix("0x") {
            let (a, s) = raw.split_once(':').unwrap_or((raw, "4"));
            let address = u32::from_str_radix(a, 16)
                .map_err(|_| LookupError(format!("bad address in {expr:?}")))?;
            let size = s
                .parse()
                .map_err(|_| LookupError(format!("bad size in {expr:?}")))?;
            return Ok(Variable {
                expression: expr.to_string(),
                address,
                size,
                type_name: format!("{size} bytes"),
            });
        }
        let end = expr.find(['.', '[']).unwrap_or(expr.len());
        let (name, mut rest) = (&expr[..end], &expr[end..]);
        let Some((mut address, mut ty)) = self.global(name) else {
            let address = self
                .symbols
                .symbol(name)
                .ok_or_else(|| LookupError(format!("no variable or symbol called {name:?}")))?;
            if !rest.is_empty() {
                return Err(LookupError(format!(
                    "{name:?} has no debug information: members and elements cannot be found"
                )));
            }
            return Ok(Variable {
                expression: expr.to_string(),
                address,
                size: 4,
                type_name: "symbol (no debug information; 4 bytes assumed)".into(),
            });
        };
        while !rest.is_empty() {
            if let Some(after) = rest.strip_prefix('.') {
                let end = after.find(['.', '[']).unwrap_or(after.len());
                let member = &after[..end];
                rest = &after[end..];
                let Shape::Struct(members) = &ty.shape else {
                    return Err(LookupError(format!("{} is not a struct or union", ty.name)));
                };
                let (_, offset, member_ty) = members
                    .iter()
                    .find(|(n, _, _)| n == member)
                    .ok_or_else(|| LookupError(format!("{} has no member {member:?}", ty.name)))?;
                address += offset;
                ty = self.resolve(*member_ty);
            } else if let Some(after) = rest.strip_prefix('[') {
                let close = after
                    .find(']')
                    .ok_or_else(|| LookupError(format!("missing ] in {expr:?}")))?;
                let index: u32 = after[..close]
                    .trim()
                    .parse()
                    .map_err(|_| LookupError(format!("bad index in {expr:?}")))?;
                rest = &after[close + 1..];
                let Shape::Array { element, count } = &ty.shape else {
                    return Err(LookupError(format!("{} is not an array", ty.name)));
                };
                if count.is_some_and(|c| index >= c) {
                    return Err(LookupError(format!(
                        "index {index} is outside {} ({} elements)",
                        ty.name,
                        count.unwrap_or(0)
                    )));
                }
                let element = self.resolve(*element);
                address += index * element.size;
                ty = element;
            } else {
                return Err(LookupError(format!("cannot read {rest:?} in {expr:?}")));
            }
        }
        Ok(Variable {
            expression: expr.to_string(),
            address,
            size: ty.size.max(1),
            type_name: ty.name,
        })
    }

    /// A global variable's address and type.
    fn global(&self, name: &str) -> Option<(u32, Type)> {
        let mut units = self.dwarf.units();
        let mut index = 0;
        while let Ok(Some(header)) = units.next() {
            let unit = self.dwarf.unit(header).ok()?;
            let mut entries = unit.entries();
            while let Ok(Some((_, entry))) = entries.next_dfs() {
                if entry.tag() != gimli::DW_TAG_variable {
                    continue;
                }
                let Some(address) = entry_address(entry) else {
                    continue;
                };
                let entry_name = self.name(&unit, entry).or_else(|| {
                    // A definition may only point at its declaration.
                    let Ok(Some(AttributeValue::UnitRef(o))) =
                        entry.attr_value(gimli::DW_AT_specification)
                    else {
                        return None;
                    };
                    let decl = unit.entry(o).ok()?;
                    self.name(&unit, &decl)
                });
                if entry_name.as_deref() != Some(name) {
                    continue;
                }
                let ty = type_ref(&unit, entry, index).or_else(|| {
                    let Ok(Some(AttributeValue::UnitRef(o))) =
                        entry.attr_value(gimli::DW_AT_specification)
                    else {
                        return None;
                    };
                    let decl = unit.entry(o).ok()?;
                    type_ref(&unit, &decl, index)
                });
                return Some((address, self.resolve(ty)));
            }
            index += 1;
        }
        None
    }

    fn name(
        &self,
        unit: &gimli::Unit<R<'a>>,
        entry: &gimli::DebuggingInformationEntry<'_, '_, R<'a>>,
    ) -> Option<String> {
        let value = entry.attr_value(gimli::DW_AT_name).ok()??;
        let s = self.dwarf.attr_string(unit, value).ok()?;
        Some(s.to_string_lossy().into_owned())
    }

    fn unit(&self, index: usize) -> Option<gimli::Unit<R<'a>>> {
        let mut units = self.dwarf.units();
        let mut i = 0;
        while let Ok(Some(header)) = units.next() {
            if i == index {
                return self.dwarf.unit(header).ok();
            }
            i += 1;
        }
        None
    }

    /// Follow a type reference to the type's name, size and shape.
    fn resolve(&self, at: TypeRef) -> Type {
        let unknown = Type {
            name: "unknown type".into(),
            size: 4,
            shape: Shape::Scalar,
        };
        let Some((index, offset)) = at else {
            return Type {
                name: "void".into(),
                ..unknown
            };
        };
        let Some(unit) = self.unit(index) else {
            return unknown;
        };
        self.resolve_in(&unit, index, offset, 0).unwrap_or(unknown)
    }

    fn resolve_in(
        &self,
        unit: &gimli::Unit<R<'a>>,
        index: usize,
        offset: UnitOffset,
        depth: u32,
    ) -> Option<Type> {
        if depth > 32 {
            return None;
        }
        let entry = unit.entry(offset).ok()?;
        let size = || match entry.attr_value(gimli::DW_AT_byte_size).ok()? {
            Some(v) => v.udata_value().map(|v| v as u32),
            None => None,
        };
        let inner = type_ref(unit, &entry, index);
        let name = self.name(unit, &entry);
        let tag = entry.tag();
        match tag {
            gimli::DW_TAG_typedef
            | gimli::DW_TAG_volatile_type
            | gimli::DW_TAG_const_type
            | gimli::DW_TAG_restrict_type => {
                let mut t = self.resolve_in(unit, index, inner?.1, depth + 1)?;
                let prefix = match tag {
                    gimli::DW_TAG_volatile_type => Some("volatile "),
                    gimli::DW_TAG_const_type => Some("const "),
                    _ => None,
                };
                t.name = match (tag, name, prefix) {
                    (gimli::DW_TAG_typedef, Some(n), _) => n,
                    (_, _, Some(p)) => format!("{p}{}", t.name),
                    _ => t.name,
                };
                Some(t)
            }
            gimli::DW_TAG_pointer_type => Some(Type {
                name: match inner.and_then(|i| self.resolve_in(unit, index, i.1, depth + 1)) {
                    Some(t) => format!("{} *", t.name),
                    None => "void *".into(),
                },
                size: size().unwrap_or(4),
                shape: Shape::Scalar,
            }),
            gimli::DW_TAG_base_type | gimli::DW_TAG_enumeration_type => Some(Type {
                name: name.unwrap_or_else(|| "enum".into()),
                size: size().unwrap_or(4),
                shape: Shape::Scalar,
            }),
            gimli::DW_TAG_structure_type | gimli::DW_TAG_union_type => {
                self.aggregate(unit, index, offset, tag, name, size())
            }
            gimli::DW_TAG_array_type => self.array(unit, index, offset, inner, depth),
            _ => None,
        }
    }

    /// A struct or union and its members.
    #[allow(clippy::too_many_arguments)]
    fn aggregate(
        &self,
        unit: &gimli::Unit<R<'a>>,
        index: usize,
        offset: UnitOffset,
        tag: gimli::DwTag,
        name: Option<String>,
        size: Option<u32>,
    ) -> Option<Type> {
        let mut members = Vec::new();
        let mut tree = unit.entries_tree(Some(offset)).ok()?;
        let root = tree.root().ok()?;
        let mut children = root.children();
        while let Ok(Some(child)) = children.next() {
            let e = child.entry();
            if e.tag() != gimli::DW_TAG_member {
                continue;
            }
            let member_offset = match e.attr_value(gimli::DW_AT_data_member_location) {
                Ok(Some(AttributeValue::Exprloc(expr))) => plus_uconst(&expr),
                Ok(Some(v)) => v.udata_value().map(|v| v as u32),
                _ => Some(0), // union members
            }
            .unwrap_or(0);
            members.push((
                self.name(unit, e).unwrap_or_default(),
                member_offset,
                type_ref(unit, e, index),
            ));
        }
        let kind = if tag == gimli::DW_TAG_union_type {
            "union"
        } else {
            "struct"
        };
        Some(Type {
            name: name.map_or_else(|| format!("{kind} <anonymous>"), |n| format!("{kind} {n}")),
            size: size.unwrap_or(0),
            shape: Shape::Struct(members),
        })
    }

    /// An array: its element type and count.
    fn array(
        &self,
        unit: &gimli::Unit<R<'a>>,
        index: usize,
        offset: UnitOffset,
        inner: TypeRef,
        depth: u32,
    ) -> Option<Type> {
        let element = self.resolve_in(unit, index, inner?.1, depth + 1)?;
        let mut count = None;
        let mut tree = unit.entries_tree(Some(offset)).ok()?;
        let root = tree.root().ok()?;
        let mut children = root.children();
        while let Ok(Some(child)) = children.next() {
            let e = child.entry();
            if e.tag() != gimli::DW_TAG_subrange_type {
                continue;
            }
            if let Ok(Some(v)) = e.attr_value(gimli::DW_AT_count) {
                count = v.udata_value().map(|v| v as u32);
            } else if let Ok(Some(v)) = e.attr_value(gimli::DW_AT_upper_bound) {
                count = v.udata_value().map(|v| v as u32 + 1);
            }
            break;
        }
        Some(Type {
            name: format!(
                "{}[{}]",
                element.name,
                count.map_or_else(String::new, |c| c.to_string())
            ),
            size: count.map_or(0, |c| c * element.size),
            shape: Shape::Array {
                element: inner,
                count,
            },
        })
    }
}

/// The address of a variable whose location is a plain `DW_OP_addr`.
fn entry_address(entry: &gimli::DebuggingInformationEntry<'_, '_, R<'_>>) -> Option<u32> {
    let Ok(Some(AttributeValue::Exprloc(expr))) = entry.attr_value(gimli::DW_AT_location) else {
        return None;
    };
    let mut r = expr.0;
    if r.read_u8().ok()? != gimli::DW_OP_addr.0 {
        return None;
    }
    let a = r.read_u32().ok()?;
    r.is_empty().then_some(a)
}

/// The offset of a `DW_OP_plus_uconst` member location.
fn plus_uconst(expr: &gimli::Expression<R<'_>>) -> Option<u32> {
    let mut r = expr.0;
    if r.read_u8().ok()? != gimli::DW_OP_plus_uconst.0 {
        return None;
    }
    r.read_uleb128().ok().map(|v| v as u32)
}

fn type_ref(
    _unit: &gimli::Unit<R<'_>>,
    entry: &gimli::DebuggingInformationEntry<'_, '_, R<'_>>,
    index: usize,
) -> TypeRef {
    match entry.attr_value(gimli::DW_AT_type).ok()? {
        Some(AttributeValue::UnitRef(o)) => Some((index, o)),
        _ => None,
    }
}
