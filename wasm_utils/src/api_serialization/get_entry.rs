use holochain_core_types::cas::content::Address;
use holochain_core_types::json::JsonString;
use serde_json;

#[derive(Deserialize, Default, Debug, Serialize)]
pub struct GetEntryArgs {
    pub address: Address,
}

#[derive(Deserialize, Debug, Serialize)]
pub enum GetResultStatus {
    Found,
    NotFound,
}

#[derive(Debug)]
pub struct GetEntryResult {
    pub status: GetResultStatus,
    pub entry_json: JsonString,
}

/// GetEntryResult is double serialized!
/// this struct facilitates outer serialization
#[derive(Serialize, Deserialize)]
pub struct SerializableGetEntryResult {
    pub status: String,
    pub entry_json: String,
}

impl GetEntryResult {
    pub fn found(entry_json: JsonString) -> GetEntryResult {
        GetEntryResult {
            status: GetResultStatus::Found,
            entry_json,
        }
    }

    pub fn not_found() -> GetEntryResult {
        GetEntryResult {
            status: GetResultStatus::NotFound,
            entry_json: JsonString::none(),
        }
    }
}

impl From<GetResultStatus> for JsonString {
    fn from(get_result_status: GetResultStatus) -> JsonString {
        JsonString::from(serde_json::to_string(&get_result_status).expect("could not Jsonify GetResultStatus"))
    }
}

impl From<JsonString> for GetResultStatus {
    fn from(json_string: JsonString) -> GetResultStatus {
        serde_json::from_str(&String::from(json_string)).expect("could not deserialize GetStatusResult")
    }
}

impl From<SerializableGetEntryResult> for JsonString {
    fn from(serializable_get_entry_result: SerializableGetEntryResult) -> JsonString {
        JsonString::from(serde_json::to_string(&serializable_get_entry_result).expect("could not Jsonify SerializableGetEntryResult"))
    }
}
