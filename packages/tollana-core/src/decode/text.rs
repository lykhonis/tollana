use super::error::DecodeError;
use super::module::{
    Export, ExportKind, Function, FunctionType, Global, GlobalInit, HostImport, Module, Mutability,
};
use crate::instruction::{BlockType, Instruction};
use crate::value::ValueType;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    LParen,
    RParen,
    Ident(String),
    String(String),
    Int(i64),
}

struct Lexer<'a> {
    src: &'a str,
    pos: usize,
}

impl<'a> Lexer<'a> {
    fn new(src: &'a str) -> Self {
        Self { src, pos: 0 }
    }

    fn peek_char(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let mut chars = self.src[self.pos..].chars();
        let c = chars.next()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn skip_ws_and_comments(&mut self) {
        loop {
            while matches!(self.peek_char(), Some(c) if c.is_whitespace()) {
                self.bump();
            }
            if self.peek_char() == Some(';') {
                while let Some(c) = self.bump() {
                    if c == '\n' {
                        break;
                    }
                }
                continue;
            }
            break;
        }
    }

    fn next_token(&mut self) -> Result<Option<Token>, DecodeError> {
        self.skip_ws_and_comments();
        let Some(c) = self.peek_char() else {
            return Ok(None);
        };
        match c {
            '(' => {
                self.bump();
                Ok(Some(Token::LParen))
            }
            ')' => {
                self.bump();
                Ok(Some(Token::RParen))
            }
            '"' => Ok(Some(Token::String(self.string()?))),
            '-' | '0'..='9' => Ok(Some(Token::Int(self.integer()?))),
            _ => Ok(Some(Token::Ident(self.ident()?))),
        }
    }

    fn ident(&mut self) -> Result<String, DecodeError> {
        let start = self.pos;
        let first = self
            .peek_char()
            .ok_or_else(|| DecodeError::new("expected ident"))?;
        if !(first.is_ascii_alphabetic() || first == '_') {
            return Err(DecodeError::new(format!("unexpected character {first:?}")));
        }
        self.bump();
        while let Some(c) = self.peek_char() {
            if c.is_ascii_alphanumeric() || c == '_' || c == '.' {
                self.bump();
            } else {
                break;
            }
        }
        let s = &self.src[start..self.pos];
        if s.contains('\0') {
            return Err(DecodeError::new("U+0000 in name"));
        }
        Ok(s.to_string())
    }

    fn integer(&mut self) -> Result<i64, DecodeError> {
        let start = self.pos;
        if self.peek_char() == Some('-') {
            self.bump();
        }
        if self.src[self.pos..].starts_with("0x") || self.src[self.pos..].starts_with("0X") {
            self.bump();
            self.bump();
            let hex_start = self.pos;
            while matches!(self.peek_char(), Some(c) if c.is_ascii_hexdigit()) {
                self.bump();
            }
            if self.pos == hex_start {
                return Err(DecodeError::new("invalid hex integer"));
            }
            let neg = self.src[start..].starts_with('-');
            let digits = &self.src[hex_start..self.pos];
            let v = u64::from_str_radix(digits, 16)
                .map_err(|_| DecodeError::new("invalid hex integer"))?;
            let signed = if neg { -(v as i64) } else { v as i64 };
            return Ok(signed);
        }
        while matches!(self.peek_char(), Some(c) if c.is_ascii_digit()) {
            self.bump();
        }
        self.src[start..self.pos]
            .parse::<i64>()
            .map_err(|_| DecodeError::new("invalid integer"))
    }

    fn string(&mut self) -> Result<String, DecodeError> {
        self.bump();
        let mut out = String::new();
        loop {
            let c = self
                .bump()
                .ok_or_else(|| DecodeError::new("unterminated string"))?;
            match c {
                '"' => break,
                '\\' => {
                    let e = self
                        .bump()
                        .ok_or_else(|| DecodeError::new("unterminated string escape"))?;
                    match e {
                        '\\' => out.push('\\'),
                        '"' => out.push('"'),
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        'u' => {
                            if self.bump() != Some('{') {
                                return Err(DecodeError::new("expected \\u{hex}"));
                            }
                            let hex_start = self.pos;
                            while matches!(self.peek_char(), Some(c) if c.is_ascii_hexdigit()) {
                                self.bump();
                            }
                            if self.bump() != Some('}') {
                                return Err(DecodeError::new("expected } in \\u{hex}"));
                            }
                            let hex = &self.src[hex_start..self.pos - 1];
                            let scalar = u32::from_str_radix(hex, 16)
                                .map_err(|_| DecodeError::new("bad unicode escape"))?;
                            let ch = char::from_u32(scalar)
                                .ok_or_else(|| DecodeError::new("bad unicode scalar"))?;
                            if ch == '\0' {
                                return Err(DecodeError::new("U+0000 in name"));
                            }
                            out.push(ch);
                        }
                        _ => return Err(DecodeError::new("unknown string escape")),
                    }
                }
                '\0' => return Err(DecodeError::new("U+0000 in name")),
                other => out.push(other),
            }
        }
        if out.contains('\0') {
            return Err(DecodeError::new("U+0000 in name"));
        }
        Ok(out)
    }
}

struct Parser<'a> {
    lexer: Lexer<'a>,
    peeked: Option<Token>,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str) -> Self {
        Self {
            lexer: Lexer::new(src),
            peeked: None,
        }
    }

    fn peek(&mut self) -> Result<Option<&Token>, DecodeError> {
        if self.peeked.is_none() {
            self.peeked = self.lexer.next_token()?;
        }
        Ok(self.peeked.as_ref())
    }

    fn bump(&mut self) -> Result<Option<Token>, DecodeError> {
        if let Some(t) = self.peeked.take() {
            return Ok(Some(t));
        }
        self.lexer.next_token()
    }

    fn expect(&mut self, want: &str) -> Result<Token, DecodeError> {
        self.bump()?
            .ok_or_else(|| DecodeError::new(format!("expected {want}, found eof")))
    }

    fn expect_lparen(&mut self) -> Result<(), DecodeError> {
        match self.expect("(")? {
            Token::LParen => Ok(()),
            other => Err(DecodeError::new(format!("expected (, found {other:?}"))),
        }
    }

    fn expect_rparen(&mut self) -> Result<(), DecodeError> {
        match self.expect(")")? {
            Token::RParen => Ok(()),
            other => Err(DecodeError::new(format!("expected ), found {other:?}"))),
        }
    }

    fn expect_ident(&mut self) -> Result<String, DecodeError> {
        match self.expect("ident")? {
            Token::Ident(s) => Ok(s),
            other => Err(DecodeError::new(format!("expected ident, found {other:?}"))),
        }
    }

    fn parse_module(&mut self) -> Result<Module, DecodeError> {
        self.expect_lparen()?;
        let kw = self.expect_ident()?;
        if kw != "module" {
            return Err(DecodeError::new("expected module"));
        }
        let mut module = Module::new();
        while !matches!(self.peek()?, Some(Token::RParen)) {
            self.parse_field(&mut module)?;
        }
        self.expect_rparen()?;
        if self.peek()?.is_some() {
            return Err(DecodeError::new("trailing tokens after module"));
        }
        Ok(module)
    }

    fn parse_field(&mut self, module: &mut Module) -> Result<(), DecodeError> {
        self.expect_lparen()?;
        let kw = self.expect_ident()?;
        match kw.as_str() {
            "memory" => {
                self.expect_lparen()?;
                if self.expect_ident()? != "pages" {
                    return Err(DecodeError::new("expected pages"));
                }
                let pages = self.expect_u32()?;
                self.expect_rparen()?;
                self.expect_rparen()?;
                if module.memory_page_count.is_some() {
                    return Err(DecodeError::new("duplicate memory"));
                }
                module.memory_page_count = Some(pages);
            }
            "host.import" => {
                let name = self.parse_name()?;
                if module.host_imports.iter().any(|h| h.name == name) {
                    return Err(DecodeError::new("duplicate host.import name"));
                }
                let mut plugin_id = None;
                let mut method_id = None;
                let mut parameters = Vec::new();
                let mut results = Vec::new();
                while !matches!(self.peek()?, Some(Token::RParen)) {
                    self.expect_lparen()?;
                    let inner = self.expect_ident()?;
                    match inner.as_str() {
                        "pluginId" => plugin_id = Some(self.expect_u32()?),
                        "methodId" => method_id = Some(self.expect_u32()?),
                        "param" => parameters.push(self.parse_value_type()?),
                        "result" => results.push(self.parse_value_type()?),
                        other => {
                            return Err(DecodeError::new(format!(
                                "unexpected host.import field {other}"
                            )));
                        }
                    }
                    self.expect_rparen()?;
                }
                self.expect_rparen()?;
                let plugin_id =
                    plugin_id.ok_or_else(|| DecodeError::new("host.import missing pluginId"))?;
                let method_id =
                    method_id.ok_or_else(|| DecodeError::new("host.import missing methodId"))?;
                let ty = FunctionType {
                    parameters,
                    results,
                };
                let type_index = intern_type(module, ty);
                module.host_imports.push(HostImport {
                    name,
                    plugin_id,
                    method_id,
                    type_index,
                });
            }
            "func" => {
                let mut export = None;
                let mut parameters = Vec::new();
                let mut results = Vec::new();
                let mut locals = Vec::new();
                while let Some(Token::LParen) = self.peek()? {
                    let saved = self.lexer.pos;
                    let saved_tok = self.peeked.clone();
                    self.expect_lparen()?;
                    let ident = match self.bump()? {
                        Some(Token::Ident(s)) => s,
                        _ => {
                            return Err(DecodeError::new("expected ident after ("));
                        }
                    };
                    match ident.as_str() {
                        "export" => {
                            export = Some(self.parse_string_token()?);
                            self.expect_rparen()?;
                        }
                        "param" => {
                            parameters.push(self.parse_value_type()?);
                            self.expect_rparen()?;
                        }
                        "result" => {
                            results.push(self.parse_value_type()?);
                            self.expect_rparen()?;
                        }
                        "local" => {
                            locals.push(self.parse_value_type()?);
                            self.expect_rparen()?;
                        }
                        _ => {
                            self.lexer.pos = saved;
                            self.peeked = saved_tok;
                            break;
                        }
                    }
                }
                let mut instructions = Vec::new();
                while !matches!(self.peek()?, Some(Token::RParen)) {
                    self.parse_instruction_form(&mut instructions, module)?;
                }
                self.expect_rparen()?;
                if instructions.last() != Some(&Instruction::End) {
                    instructions.push(Instruction::End);
                }
                let ty = FunctionType {
                    parameters,
                    results,
                };
                let type_index = intern_type(module, ty);
                let index = module.functions.len() as u32;
                module.functions.push(Function {
                    type_index,
                    locals,
                    instructions,
                });
                if let Some(name) = export {
                    module.exports.push(Export {
                        name,
                        kind: ExportKind::Function,
                        index,
                    });
                }
            }
            "global" => {
                let mut mutability = Mutability::Immutable;
                if matches!(self.peek()?, Some(Token::LParen)) {
                    let saved = self.lexer.pos;
                    let saved_tok = self.peeked.clone();
                    self.expect_lparen()?;
                    if let Some(Token::Ident(s)) = self.peek()?.cloned() {
                        if s == "mutable" {
                            self.bump()?;
                            self.expect_rparen()?;
                            mutability = Mutability::Mutable;
                        } else {
                            self.lexer.pos = saved;
                            self.peeked = saved_tok;
                        }
                    } else {
                        self.lexer.pos = saved;
                        self.peeked = saved_tok;
                    }
                }
                let value_type = self.parse_value_type()?;
                let init = if matches!(self.peek()?, Some(Token::LParen)) {
                    let saved = self.lexer.pos;
                    let saved_tok = self.peeked.clone();
                    self.expect_lparen()?;
                    if let Some(Token::Ident(s)) = self.peek()?.cloned() {
                        if s == "host.injected" {
                            self.bump()?;
                            self.expect_rparen()?;
                            GlobalInit::HostInjected
                        } else {
                            self.lexer.pos = saved;
                            self.peeked = saved_tok;
                            let mut insts = Vec::new();
                            while !matches!(self.peek()?, Some(Token::RParen)) {
                                self.parse_instruction_form(&mut insts, module)?;
                            }
                            if insts.last() != Some(&Instruction::End) {
                                insts.push(Instruction::End);
                            }
                            GlobalInit::ConstantExpression(insts)
                        }
                    } else {
                        self.lexer.pos = saved;
                        self.peeked = saved_tok;
                        let mut insts = Vec::new();
                        while !matches!(self.peek()?, Some(Token::RParen)) {
                            self.parse_instruction_form(&mut insts, module)?;
                        }
                        if insts.last() != Some(&Instruction::End) {
                            insts.push(Instruction::End);
                        }
                        GlobalInit::ConstantExpression(insts)
                    }
                } else {
                    let mut insts = Vec::new();
                    while !matches!(self.peek()?, Some(Token::RParen)) {
                        self.parse_instruction_form(&mut insts, module)?;
                    }
                    if insts.last() != Some(&Instruction::End) {
                        insts.push(Instruction::End);
                    }
                    GlobalInit::ConstantExpression(insts)
                };
                self.expect_rparen()?;
                module.globals.push(Global {
                    value_type,
                    mutability,
                    init,
                });
            }
            other => {
                return Err(DecodeError::new(format!("unknown module field {other}")));
            }
        }
        Ok(())
    }

    fn parse_instruction_form(
        &mut self,
        out: &mut Vec<Instruction>,
        module: &Module,
    ) -> Result<(), DecodeError> {
        if matches!(self.peek()?, Some(Token::LParen)) {
            self.parse_folded(out, module)
        } else {
            let inst = self.parse_unfolded(module)?;
            out.push(inst);
            Ok(())
        }
    }

    fn parse_unfolded(&mut self, module: &Module) -> Result<Instruction, DecodeError> {
        let name = self.expect_ident()?;
        self.instruction_from_name(&name, module, false)
    }

    fn parse_folded(
        &mut self,
        out: &mut Vec<Instruction>,
        module: &Module,
    ) -> Result<(), DecodeError> {
        self.expect_lparen()?;
        let name = self.expect_ident()?;
        match name.as_str() {
            "block" | "loop" | "if" => {
                let bt = self.parse_block_type_opt()?;
                let op = match name.as_str() {
                    "block" => Instruction::Block { block_type: bt },
                    "loop" => Instruction::Loop { block_type: bt },
                    _ => Instruction::If { block_type: bt },
                };
                out.push(op);
                if name == "if" {
                    loop {
                        match self.peek()? {
                            Some(Token::RParen) => break,
                            Some(Token::Ident(s)) if s == "else" => break,
                            _ => self.parse_instruction_form(out, module)?,
                        }
                    }
                    if let Some(Token::Ident(s)) = self.peek()? {
                        if s == "else" {
                            self.bump()?;
                            out.push(Instruction::Else);
                            while !matches!(self.peek()?, Some(Token::RParen)) {
                                self.parse_instruction_form(out, module)?;
                            }
                        }
                    }
                } else {
                    while !matches!(self.peek()?, Some(Token::RParen)) {
                        self.parse_instruction_form(out, module)?;
                    }
                }
                self.expect_rparen()?;
                out.push(Instruction::End);
                Ok(())
            }
            _ => {
                let inst = self.instruction_from_name(&name, module, true)?;
                while !matches!(self.peek()?, Some(Token::RParen)) {
                    self.parse_instruction_form(out, module)?;
                }
                self.expect_rparen()?;
                out.push(inst);
                Ok(())
            }
        }
    }

    fn parse_block_type_opt(&mut self) -> Result<BlockType, DecodeError> {
        match self.peek()? {
            Some(Token::Ident(s)) if s == "empty" => {
                self.bump()?;
                Ok(BlockType::Empty)
            }
            Some(Token::LParen) => {
                let saved = self.lexer.pos;
                let saved_tok = self.peeked.clone();
                self.expect_lparen()?;
                match self.bump()? {
                    Some(Token::Ident(s)) if s == "result" => {
                        let t = self.parse_value_type()?;
                        self.expect_rparen()?;
                        Ok(BlockType::SingleResult(t))
                    }
                    Some(Token::Ident(s)) if s == "typeIndex" => {
                        let i = self.expect_u32()?;
                        self.expect_rparen()?;
                        Ok(BlockType::TypeIndex(i))
                    }
                    _ => {
                        self.lexer.pos = saved;
                        self.peeked = saved_tok;
                        Ok(BlockType::Empty)
                    }
                }
            }
            _ => Ok(BlockType::Empty),
        }
    }

    fn instruction_from_name(
        &mut self,
        name: &str,
        module: &Module,
        folded: bool,
    ) -> Result<Instruction, DecodeError> {
        match name {
            "nop" => Ok(Instruction::Nop),
            "unreachable" => Ok(Instruction::Unreachable),
            "drop" => Ok(Instruction::Drop),
            "else" => Ok(Instruction::Else),
            "end" => Ok(Instruction::End),
            "return" => Ok(Instruction::Return),
            "memory.size" => Ok(Instruction::MemorySize),
            "i32.add" => Ok(Instruction::I32Add),
            "i32.sub" => Ok(Instruction::I32Sub),
            "i32.mul" => Ok(Instruction::I32Mul),
            "i32.div_s" => Ok(Instruction::I32DivS),
            "i32.rem_s" => Ok(Instruction::I32RemS),
            "i32.eqz" => Ok(Instruction::I32Eqz),
            "i32.eq" => Ok(Instruction::I32Eq),
            "i32.ne" => Ok(Instruction::I32Ne),
            "i32.lt_s" => Ok(Instruction::I32LtS),
            "i32.gt_s" => Ok(Instruction::I32GtS),
            "i32.le_s" => Ok(Instruction::I32LeS),
            "i32.ge_s" => Ok(Instruction::I32GeS),
            "i64.add" => Ok(Instruction::I64Add),
            "i64.sub" => Ok(Instruction::I64Sub),
            "i64.mul" => Ok(Instruction::I64Mul),
            "i64.div_s" => Ok(Instruction::I64DivS),
            "i64.rem_s" => Ok(Instruction::I64RemS),
            "i64.eqz" => Ok(Instruction::I64Eqz),
            "i64.eq" => Ok(Instruction::I64Eq),
            "i64.ne" => Ok(Instruction::I64Ne),
            "i64.lt_s" => Ok(Instruction::I64LtS),
            "i64.gt_s" => Ok(Instruction::I64GtS),
            "i64.le_s" => Ok(Instruction::I64LeS),
            "i64.ge_s" => Ok(Instruction::I64GeS),
            "i32.const" => Ok(Instruction::I32Const {
                value: self.expect_i32()?,
            }),
            "i64.const" => Ok(Instruction::I64Const {
                value: self.expect_i64()?,
            }),
            "local.get" => Ok(Instruction::LocalGet {
                local_index: self.expect_u32()?,
            }),
            "local.set" => Ok(Instruction::LocalSet {
                local_index: self.expect_u32()?,
            }),
            "local.tee" => Ok(Instruction::LocalTee {
                local_index: self.expect_u32()?,
            }),
            "global.get" => Ok(Instruction::GlobalGet {
                global_index: self.expect_u32()?,
            }),
            "global.set" => Ok(Instruction::GlobalSet {
                global_index: self.expect_u32()?,
            }),
            "br" => Ok(Instruction::Br {
                label_depth: self.expect_u32()?,
            }),
            "br_if" => Ok(Instruction::BrIf {
                label_depth: self.expect_u32()?,
            }),
            "call" => Ok(Instruction::Call {
                function_index: self.expect_u32()?,
            }),
            "i32.load" => Ok(Instruction::I32Load {
                immediate_offset: self.parse_offset(folded)?,
            }),
            "i32.store" => Ok(Instruction::I32Store {
                immediate_offset: self.parse_offset(folded)?,
            }),
            "i64.load" => Ok(Instruction::I64Load {
                immediate_offset: self.parse_offset(folded)?,
            }),
            "i64.store" => Ok(Instruction::I64Store {
                immediate_offset: self.parse_offset(folded)?,
            }),
            "host.invoke" => {
                let import_name = self.parse_name()?;
                let host_import_index = module
                    .host_imports
                    .iter()
                    .position(|h| h.name == import_name)
                    .ok_or_else(|| DecodeError::new(format!("unknown host.import {import_name}")))?
                    as u32;
                Ok(Instruction::HostInvoke { host_import_index })
            }
            "block" => Ok(Instruction::Block {
                block_type: self.parse_block_type_opt()?,
            }),
            "loop" => Ok(Instruction::Loop {
                block_type: self.parse_block_type_opt()?,
            }),
            "if" => Ok(Instruction::If {
                block_type: self.parse_block_type_opt()?,
            }),
            other => Err(DecodeError::new(format!("unknown instruction {other}"))),
        }
    }

    fn parse_offset(&mut self, folded: bool) -> Result<u32, DecodeError> {
        if matches!(self.peek()?, Some(Token::LParen)) {
            let saved = self.lexer.pos;
            let saved_tok = self.peeked.clone();
            self.expect_lparen()?;
            if let Some(Token::Ident(s)) = self.peek()?.cloned() {
                if s == "offset" {
                    self.bump()?;
                    let n = self.expect_u32()?;
                    self.expect_rparen()?;
                    return Ok(n);
                }
            }
            self.lexer.pos = saved;
            self.peeked = saved_tok;
            return Ok(0);
        }
        if !folded {
            if let Some(Token::Int(n)) = self.peek()? {
                let n = *n;
                self.bump()?;
                return u32_from_i64(n);
            }
        }
        Ok(0)
    }

    fn parse_value_type(&mut self) -> Result<ValueType, DecodeError> {
        let s = self.expect_ident()?;
        match s.as_str() {
            "i32" => Ok(ValueType::I32),
            "i64" => Ok(ValueType::I64),
            "unit" => Ok(ValueType::Unit),
            "Capability" | "capability" => Ok(ValueType::Capability),
            other => Err(DecodeError::new(format!("unknown type {other}"))),
        }
    }

    fn parse_name(&mut self) -> Result<String, DecodeError> {
        match self.expect("name")? {
            Token::Ident(s) | Token::String(s) => {
                if s.contains('\0') {
                    Err(DecodeError::new("U+0000 in name"))
                } else {
                    Ok(s)
                }
            }
            other => Err(DecodeError::new(format!("expected name, found {other:?}"))),
        }
    }

    fn parse_string_token(&mut self) -> Result<String, DecodeError> {
        match self.expect("string")? {
            Token::String(s) => {
                if s.contains('\0') {
                    Err(DecodeError::new("U+0000 in name"))
                } else {
                    Ok(s)
                }
            }
            other => Err(DecodeError::new(format!(
                "expected string, found {other:?}"
            ))),
        }
    }

    fn expect_u32(&mut self) -> Result<u32, DecodeError> {
        match self.expect("u32")? {
            Token::Int(n) => u32_from_i64(n),
            other => Err(DecodeError::new(format!("expected u32, found {other:?}"))),
        }
    }

    fn expect_i32(&mut self) -> Result<i32, DecodeError> {
        match self.expect("i32")? {
            Token::Int(n) => i32::try_from(n).map_err(|_| DecodeError::new("i32 out of range")),
            other => Err(DecodeError::new(format!("expected i32, found {other:?}"))),
        }
    }

    fn expect_i64(&mut self) -> Result<i64, DecodeError> {
        match self.expect("i64")? {
            Token::Int(n) => Ok(n),
            other => Err(DecodeError::new(format!("expected i64, found {other:?}"))),
        }
    }
}

fn intern_type(module: &mut Module, ty: FunctionType) -> u32 {
    if let Some(i) = module.types.iter().position(|t| *t == ty) {
        return i as u32;
    }
    module.types.push(ty);
    (module.types.len() - 1) as u32
}

fn u32_from_i64(n: i64) -> Result<u32, DecodeError> {
    u32::try_from(n).map_err(|_| DecodeError::new("u32 out of range"))
}

pub fn decode_text(src: &str) -> Result<Module, DecodeError> {
    if src.as_bytes().contains(&0) {
        return Err(DecodeError::new("U+0000 in name"));
    }
    let mut p = Parser::new(src);
    p.parse_module()
}
