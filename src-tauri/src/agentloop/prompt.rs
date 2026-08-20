//! Native 会话历史加载与上下文预算边界。
//!
//! SQLite 中的真实 user/assistant 消息直接转换成 Provider item；不拼接说话人标签，
//! 也不注入整个项目状态。当前项目事实只能由观察工具提供。

use rusqlite::{params, Connection};
use serde_json::{json, Value};

const MAX_HISTORY_MESSAGES: usize = 12;
const MAX_HISTORY_CHARS: usize = 8_000;

pub(super) fn load_native_message_history(
    connection: &Connection,
    conversation_id: &str,
    editing_task_id: &str,
    exclude_request: &str,
) -> Vec<Value> {
    let mut statement = match connection.prepare(
        "SELECT messages.role, messages.content FROM messages
         JOIN conversations ON conversations.id = messages.conversation_id
         WHERE messages.conversation_id = ?1
           AND conversations.editing_task_id = ?2
           AND messages.role IN ('user', 'assistant', 'agent')
         ORDER BY messages.created_at DESC, messages.id DESC LIMIT ?3",
    ) {
        Ok(statement) => statement,
        Err(_) => return Vec::new(),
    };
    let rows = match statement.query_map(
        params![
            conversation_id,
            editing_task_id,
            MAX_HISTORY_MESSAGES as i64 + 1
        ],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    ) {
        Ok(rows) => rows,
        Err(_) => return Vec::new(),
    };
    let mut newest_first = Vec::new();
    let mut skipped_current = false;
    let mut total_chars = 0;
    for row in rows.filter_map(Result::ok) {
        let (role, content) = row;
        if !skipped_current && role == "user" && content.trim() == exclude_request.trim() {
            skipped_current = true;
            continue;
        }
        let role = match role.as_str() {
            "user" => "user",
            "assistant" | "agent" => "assistant",
            _ => continue,
        };
        let chars = content.chars().count();
        if total_chars + chars > MAX_HISTORY_CHARS {
            continue;
        }
        total_chars += chars;
        let content_type = if role == "assistant" {
            "output_text"
        } else {
            "input_text"
        };
        newest_first.push(json!({
            "role": role,
            "content": [{"type": content_type, "text": content}],
        }));
    }
    newest_first.reverse();
    newest_first
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_history_uses_real_roles_without_speaker_labels() {
        let connection = Connection::open_in_memory().expect("open history database");
        crate::db::migrate(&connection).expect("migrate history database");
        connection
            .execute_batch(
                "INSERT INTO projects (id, name, created_at, updated_at) VALUES ('p', 'Project', 1, 1);
                 INSERT INTO editing_tasks (id, project_id, title, brief, created_at, updated_at) VALUES ('t', 'p', 'Task', '', 1, 1);
                 INSERT INTO conversations (id, project_id, editing_task_id, title, status, created_at, updated_at) VALUES ('c', 'p', 't', 'Conversation', 'ready', 1, 1);
                 INSERT INTO messages (id, conversation_id, role, content, created_at) VALUES
                   ('u1', 'c', 'user', '现在有多少素材？', 2),
                   ('a1', 'c', 'assistant', '项目中有 10 个素材。', 3),
                   ('u2', 'c', 'user', '现在有多少素材？', 4);",
            )
            .expect("seed history messages");

        let history = load_native_message_history(&connection, "c", "t", "现在有多少素材？");

        assert_eq!(history.len(), 2);
        assert_eq!(history[0]["role"], "user");
        assert_eq!(history[1]["role"], "assistant");
        assert_eq!(history[0]["content"][0]["text"], "现在有多少素材？");
        assert_eq!(history[1]["content"][0]["text"], "项目中有 10 个素材。");
        assert!(!history.iter().any(|item| {
            item.to_string().contains("用户：") || item.to_string().contains("助手：")
        }));
    }
}
