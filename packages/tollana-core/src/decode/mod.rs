mod binary;
mod error;
mod module;
mod text;

pub use binary::encode_binary;
pub use error::DecodeError;
pub use module::{
    Export, ExportKind, Function, FunctionType, Global, GlobalInit, HostImport, Module, Mutability,
};

pub fn decode_binary(bytes: &[u8]) -> Result<Module, DecodeError> {
    binary::decode_binary(bytes)
}

pub fn decode_text(src: &str) -> Result<Module, DecodeError> {
    text::decode_text(src)
}

pub fn decode(bytes: &[u8]) -> Result<Module, DecodeError> {
    if bytes.starts_with(b"TIR\0") {
        decode_binary(bytes)
    } else {
        let src = std::str::from_utf8(bytes).map_err(|_| DecodeError::new("text is not UTF-8"))?;
        decode_text(src)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instruction::Instruction;

    fn parse_hex(s: &str) -> Vec<u8> {
        s.split_whitespace()
            .filter(|t| !t.starts_with(';'))
            .flat_map(|t| {
                t.as_bytes()
                    .chunks(2)
                    .filter(|c| c.len() == 2 && c[0].is_ascii_hexdigit())
                    .map(|c| u8::from_str_radix(std::str::from_utf8(c).unwrap(), 16).unwrap())
            })
            .collect()
    }

    const ADD_HEX: &str = "
54 49 52 00 01 00 00 00
01 0D 00 00 00 01 00 00 00 00 00 00 00 01 00 00 00 01
03 08 00 00 00 01 00 00 00 00 00 00 00
06 11 00 00 00 01 00 00 00 04 00 00 00 6D 61 69 6E 01 00 00 00 00
07 18 00 00 00 01 00 00 00 10 00 00 00 00 00 00 00
10 01 00 00 00 10 02 00 00 00 20 64
";

    const ADD_TIR: &str = r#"
(module
  (func (export "main") (result i32)
    (i32.add (i32.const 1) (i32.const 2))))
"#;

    const ADD_TIR_UNFOLDED: &str = r#"
(module
  (func (export "main") (result i32)
    i32.const 1
    i32.const 2
    i32.add))
"#;

    const ECHO_HEX: &str = "
54 49 52 00 01 00 00 00
01 17 00 00 00
02 00 00 00
01 00 00 00 01 01 00 00 00 01
00 00 00 00 01 00 00 00 01
02 18 00 00 00
01 00 00 00
04 00 00 00 45 63 68 6F
00 00 00 00
00 00 00 00
00 00 00 00
03 08 00 00 00
01 00 00 00 01 00 00 00
06 11 00 00 00
01 00 00 00 04 00 00 00 6D 61 69 6E 01 00 00 00 00
07 17 00 00 00
01 00 00 00
0F 00 00 00
00 00 00 00
10 29 00 00 00
70 00 00 00 00
64
";

    const ECHO_TIR: &str = r#"
(module
  (host.import Echo
    (pluginId 0)
    (methodId 0)
    (param i32)
    (result i32))
  (func (export "main") (result i32)
    (host.invoke Echo (i32.const 41))))
"#;

    fn add_instructions() -> Vec<Instruction> {
        vec![
            Instruction::I32Const { value: 1 },
            Instruction::I32Const { value: 2 },
            Instruction::I32Add,
            Instruction::End,
        ]
    }

    #[test]
    fn decode_add_tirb() {
        let bytes = parse_hex(ADD_HEX);
        assert_eq!(bytes.len(), 90);
        let m = decode_binary(&bytes).expect("decode add tirb");
        assert_eq!(m.functions[0].instructions, add_instructions());
        assert_eq!(encode_binary(&m), bytes);
    }

    #[test]
    fn decode_add_tir_folded_and_unfolded() {
        let folded = decode_text(ADD_TIR).expect("folded");
        let unfolded = decode_text(ADD_TIR_UNFOLDED).expect("unfolded");
        assert_eq!(folded.functions[0].instructions, add_instructions());
        assert_eq!(
            folded.functions[0].instructions,
            unfolded.functions[0].instructions
        );
        let from_hex = decode_binary(&parse_hex(ADD_HEX)).unwrap();
        assert_eq!(
            folded.functions[0].instructions,
            from_hex.functions[0].instructions
        );
    }

    #[test]
    fn decode_echo_tirb_and_tir() {
        let bytes = parse_hex(ECHO_HEX);
        let bin = decode_binary(&bytes).expect("echo tirb");
        assert_eq!(
            bin.functions[0].instructions,
            vec![
                Instruction::I32Const { value: 41 },
                Instruction::HostInvoke {
                    host_import_index: 0
                },
                Instruction::End,
            ]
        );
        assert_eq!(encode_binary(&bin), bytes);
        let text = decode_text(ECHO_TIR).expect("echo tir");
        assert_eq!(
            text.functions[0].instructions,
            bin.functions[0].instructions
        );
        assert_eq!(text.host_imports[0].name, "Echo");
        assert_eq!(text.host_imports[0].type_index, 0);
        assert_eq!(text.functions[0].type_index, 1);
    }

    #[test]
    fn unknown_opcode_is_rejected() {
        let mut bytes = parse_hex(ADD_HEX);
        let len = bytes.len();
        bytes[len - 2] = 0xFF;
        assert!(decode_binary(&bytes).is_err());
    }

    #[test]
    fn null_in_export_name_is_rejected() {
        let src = "(module (func (export \"ma\0in\") (result i32) i32.const 1))";
        assert!(decode_text(src).is_err());
    }

    #[test]
    fn invalid_utf8_name_in_binary_is_rejected() {
        let mut bytes = parse_hex(ADD_HEX);
        let pos = bytes.windows(4).position(|w| w == [0x6D, 0x61, 0x69, 0x6E]);
        let pos = pos.expect("main");
        bytes[pos] = 0xFF;
        assert!(decode_binary(&bytes).is_err());
    }
}
