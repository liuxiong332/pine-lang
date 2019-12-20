#[derive(Debug, PartialEq, Clone)]
pub enum RuntimeErr {
    NotCompatible(String),
    NotValidParam,
    NotSupportOperator,
    NotImplement(String),
    OutBound,
    NameDeclared,
    InvalidTypeCast,
    InvalidNADeclarer,

    VarNotFound,            // The variable not found in context
    InvalidVarType(String), // The variable type is invalid.
    NameNotDeclard,

    TypeMismatch(String), // The derised type is not the same as the real type.
    TupleMismatch,        // count of Left and Right side of assignment is not the same

    Continue,
    Break,

    ForRangeIndexIsNA, // The index of for-range is na
}
