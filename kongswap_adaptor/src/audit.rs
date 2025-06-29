use sns_treasury_manager::AuditTrail;

pub fn serialize_audit_trail(audit_trail: &AuditTrail) -> Result<String, String> {
    serde_json::to_string(&audit_trail.transactions).map_err(|err| format!("{err:?}"))
}
