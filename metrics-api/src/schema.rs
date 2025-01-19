// @generated automatically by Diesel CLI.
diesel::table! {
    experiments (name) {
        name -> VarChar,
        description -> VarChar,
        timestamp -> Timestamp,
    }
}
