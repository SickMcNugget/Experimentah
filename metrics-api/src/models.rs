use diesel::prelude::*;
use std::collections::HashMap;

#[derive(Queryable, Selectable)]
#[diesel(table_name = crate::schema::experiments)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Experiment {
    name: String,
    description: String,
    // experiment_config: HashMap<String, String>,
    // environment_config: HashMap<String, String>,
    timestamp: std::time::SystemTime,
}
