// GBNF grammar enforcing the tool-call JSON schema (plan §5).
// This is why llama.cpp was chosen over MLX: sampling is constrained so the
// output is well-formed JSON matching the contract 100% of the time (plan §10).

enum ToolGrammar {
    /// Forces: {"speech": <string>, "actions": [<0..3 known tool calls>]}
    static let gbnf = #"""
    root ::= "{" ws "\"speech\"" ws ":" ws string "," ws "\"actions\"" ws ":" ws actions ws "}"

    actions ::= "[" ws "]" | "[" ws action ("," ws action)? ("," ws action)? ws "]"

    action ::= log-entry | set-current-task | update-task | open-thread | close-thread | schedule-checkin | none-action

    log-entry ::= "{" ws "\"tool\"" ws ":" ws "\"log_entry\"" "," ws "\"args\"" ws ":" ws "{" ws "\"summary\"" ws ":" ws string "," ws "\"mood\"" ws ":" ws (string | "null") ws "}" ws "}"

    set-current-task ::= "{" ws "\"tool\"" ws ":" ws "\"set_current_task\"" "," ws "\"args\"" ws ":" ws "{" ws "\"title\"" ws ":" ws string ws "}" ws "}"

    update-task ::= "{" ws "\"tool\"" ws ":" ws "\"update_task\"" "," ws "\"args\"" ws ":" ws "{" ws "\"task_id\"" ws ":" ws int "," ws "\"status\"" ws ":" ws status ws "}" ws "}"

    status ::= "\"active\"" | "\"paused\"" | "\"done\"" | "\"dropped\""

    open-thread ::= "{" ws "\"tool\"" ws ":" ws "\"open_thread\"" "," ws "\"args\"" ws ":" ws "{" ws "\"note\"" ws ":" ws string ws "}" ws "}"

    close-thread ::= "{" ws "\"tool\"" ws ":" ws "\"close_thread\"" "," ws "\"args\"" ws ":" ws "{" ws "\"thread_id\"" ws ":" ws int ws "}" ws "}"

    schedule-checkin ::= "{" ws "\"tool\"" ws ":" ws "\"schedule_checkin\"" "," ws "\"args\"" ws ":" ws "{" ws "\"minutes_from_now\"" ws ":" ws int ws "}" ws "}"

    none-action ::= "{" ws "\"tool\"" ws ":" ws "\"none\"" "," ws "\"args\"" ws ":" ws "{" ws "}" ws "}"

    string ::= "\"" char* "\""
    char ::= [^"\\\x00-\x1F] | "\\" (["\\/bfnrt] | "u" [0-9a-fA-F] [0-9a-fA-F] [0-9a-fA-F] [0-9a-fA-F])
    int ::= [0-9] [0-9]*
    ws ::= [ \t\n]*
    """#
}
