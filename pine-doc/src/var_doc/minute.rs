use crate::{DocBase, VarType};

pub fn gen_doc() -> Vec<DocBase> {
    vec![
        DocBase {
            var_type: VarType::Variable,
            name: "minute",
            signatures: vec![],
            description: "Current bar minute in exchange timezone.",
            example: "",
            returns: "",
            arguments: "",
            remarks: "",
            links: "",
        },
        DocBase {
            var_type: VarType::Function,
            name: "minute",
            signatures: vec![],
            description: "",
            example: "",
            returns: "Minute (in exchange timezone) for provided UNIX time.",
            arguments: "**time (series(int))** UNIX time in milliseconds.",
            remarks: "UNIX time is the number of milliseconds that have elapsed since 00:00:00 UTC, 1 January 1970.",
            links: "",
        },
    ]
}
