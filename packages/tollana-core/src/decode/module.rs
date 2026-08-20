use crate::instruction::Instruction;
use crate::value::ValueType;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FunctionType {
    pub parameters: Vec<ValueType>,
    pub results: Vec<ValueType>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostImport {
    pub name: String,
    pub plugin_id: u32,
    pub method_id: u32,
    pub type_index: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mutability {
    Immutable,
    Mutable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GlobalInit {
    ConstantExpression(Vec<Instruction>),
    HostInjected,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Global {
    pub value_type: ValueType,
    pub mutability: Mutability,
    pub init: GlobalInit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportKind {
    Function,
    Memory,
    Global,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Export {
    pub name: String,
    pub kind: ExportKind,
    pub index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Function {
    pub type_index: u32,
    pub locals: Vec<ValueType>,
    pub instructions: Vec<Instruction>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Module {
    pub types: Vec<FunctionType>,
    pub host_imports: Vec<HostImport>,
    pub functions: Vec<Function>,
    pub memory_page_count: Option<u32>,
    pub globals: Vec<Global>,
    pub exports: Vec<Export>,
}

impl Module {
    pub fn new() -> Self {
        Self {
            types: Vec::new(),
            host_imports: Vec::new(),
            functions: Vec::new(),
            memory_page_count: None,
            globals: Vec::new(),
            exports: Vec::new(),
        }
    }
}

impl Default for Module {
    fn default() -> Self {
        Self::new()
    }
}
