#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ValueType {
    I32 = 0x01,
    I64 = 0x02,
    Unit = 0x03,
    Capability = 0x04,
}

impl ValueType {
    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            0x01 => Some(ValueType::I32),
            0x02 => Some(ValueType::I64),
            0x03 => Some(ValueType::Unit),
            0x04 => Some(ValueType::Capability),
            _ => None,
        }
    }

    pub fn code(self) -> u8 {
        self as u8
    }

    pub fn name(self) -> &'static str {
        match self {
            ValueType::I32 => "i32",
            ValueType::I64 => "i64",
            ValueType::Unit => "unit",
            ValueType::Capability => "Capability",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Label {
    Public = 0,
    Internal = 1,
    Confidential = 2,
    Secret = 3,
}

impl Label {
    pub fn join(self, other: Self) -> Self {
        if self >= other {
            self
        } else {
            other
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CapHandle {
    pub table_index: u32,
    pub generation: u32,
}

impl CapHandle {
    pub const NULL: Self = Self {
        table_index: 0,
        generation: 0,
    };

    pub fn is_null(self) -> bool {
        self == Self::NULL
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValuePayload {
    I32(i32),
    I64(i64),
    Unit,
    Capability(CapHandle),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Value {
    pub payload: ValuePayload,
    pub label: Label,
}

impl Value {
    pub fn i32(bits: i32, label: Label) -> Self {
        Self {
            payload: ValuePayload::I32(bits),
            label,
        }
    }

    pub fn i64(bits: i64, label: Label) -> Self {
        Self {
            payload: ValuePayload::I64(bits),
            label,
        }
    }

    pub fn unit(label: Label) -> Self {
        Self {
            payload: ValuePayload::Unit,
            label,
        }
    }

    pub fn capability(handle: CapHandle, label: Label) -> Self {
        Self {
            payload: ValuePayload::Capability(handle),
            label,
        }
    }

    pub fn value_type(self) -> ValueType {
        match self.payload {
            ValuePayload::I32(_) => ValueType::I32,
            ValuePayload::I64(_) => ValueType::I64,
            ValuePayload::Unit => ValueType::Unit,
            ValuePayload::Capability(_) => ValueType::Capability,
        }
    }

    pub fn as_i32(self) -> Option<i32> {
        match self.payload {
            ValuePayload::I32(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_i64(self) -> Option<i64> {
        match self.payload {
            ValuePayload::I64(v) => Some(v),
            _ => None,
        }
    }

    pub fn default_for(ty: ValueType) -> Self {
        match ty {
            ValueType::I32 => Self::i32(0, Label::Public),
            ValueType::I64 => Self::i64(0, Label::Public),
            ValueType::Unit => Self::unit(Label::Public),
            ValueType::Capability => Self::capability(CapHandle::NULL, Label::Public),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_join_is_least_upper_bound() {
        assert_eq!(Label::Public.join(Label::Public), Label::Public);
        assert_eq!(Label::Public.join(Label::Secret), Label::Secret);
        assert_eq!(Label::Secret.join(Label::Public), Label::Secret);
        assert_eq!(
            Label::Internal.join(Label::Confidential),
            Label::Confidential
        );
        assert_eq!(
            Label::Confidential.join(Label::Internal),
            Label::Confidential
        );
    }

    #[test]
    fn null_handle_is_zero_zero() {
        assert_eq!(
            CapHandle::NULL,
            CapHandle {
                table_index: 0,
                generation: 0
            }
        );
        assert!(CapHandle::NULL.is_null());
        assert!(!CapHandle {
            table_index: 1,
            generation: 1
        }
        .is_null());
    }

    #[test]
    fn cap_handle_equality_uses_index_and_generation() {
        let a = CapHandle {
            table_index: 1,
            generation: 1,
        };
        let b = CapHandle {
            table_index: 1,
            generation: 2,
        };
        let c = CapHandle {
            table_index: 1,
            generation: 1,
        };
        assert_ne!(a, b);
        assert_eq!(a, c);
    }

    #[test]
    fn value_type_matches_payload() {
        assert_eq!(Value::i32(3, Label::Public).value_type(), ValueType::I32);
        assert_eq!(Value::i64(1, Label::Public).value_type(), ValueType::I64);
        assert_eq!(Value::unit(Label::Public).value_type(), ValueType::Unit);
        assert_eq!(
            Value::capability(CapHandle::NULL, Label::Public).value_type(),
            ValueType::Capability
        );
    }
}
