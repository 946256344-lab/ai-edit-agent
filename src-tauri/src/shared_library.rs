//! 全局子素材库与项目使用范围：源文件、素材 ID 和分析结果不复制。
use crate::db::open_connection;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::Path;
use tauri::AppHandle;
use uuid::Uuid;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedLibrary {
    id: String,
    name: String,
    asset_count: i64,
}

pub(crate) fn attach_folder(
    connection: &Connection,
    project_id: &str,
    folder: Option<&str>,
) -> Result<(), String> {
    let key = folder.unwrap_or("");
    let name = folder
        .and_then(|value| Path::new(value).file_name())
        .and_then(|value| value.to_str())
        .unwrap_or("未归类素材");
    connection
        .execute(
            "INSERT OR IGNORE INTO shared_libraries (id, source_key, name) VALUES (?1, ?2, ?3)",
            params![Uuid::new_v4().to_string(), key, name],
        )
        .map_err(|error| error.to_string())?;
    connection.execute("INSERT OR IGNORE INTO project_libraries (project_id, library_id) SELECT ?1, id FROM shared_libraries WHERE source_key = ?2", params![project_id, key]).map_err(|error| error.to_string())?;
    Ok(())
}

pub(crate) fn migrate(connection: &Connection) -> Result<(), String> {
    connection.execute_batch("CREATE TABLE IF NOT EXISTS shared_libraries (id TEXT PRIMARY KEY NOT NULL, source_key TEXT NOT NULL UNIQUE, name TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS project_libraries (project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE, library_id TEXT NOT NULL REFERENCES shared_libraries(id), PRIMARY KEY(project_id, library_id));
        CREATE VIEW IF NOT EXISTS project_asset_access AS
          SELECT project_id, id AS asset_id FROM assets
          UNION
          SELECT p.project_id, a.id FROM project_libraries p JOIN shared_libraries l ON l.id = p.library_id JOIN assets a ON coalesce(a.folder_reference, '') = l.source_key;
    ").map_err(|error| error.to_string())?;
    let migrated: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = 18)",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if !migrated {
        let transaction = connection
            .unchecked_transaction()
            .map_err(|error| error.to_string())?;
        let folders = {
            let mut statement = transaction
                .prepare("SELECT DISTINCT project_id, folder_reference FROM assets")
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
                })
                .map_err(|error| error.to_string())?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?
        };
        for (_, folder) in folders {
            let key = folder.as_deref().unwrap_or("");
            let name = folder
                .as_deref()
                .and_then(|value| Path::new(value).file_name())
                .and_then(|value| value.to_str())
                .unwrap_or("未归类素材");
            transaction.execute("INSERT OR IGNORE INTO shared_libraries (id, source_key, name) VALUES (?1, ?2, ?3)", params![Uuid::new_v4().to_string(), key, name]).map_err(|error| error.to_string())?;
        }
        transaction
            .execute(
                "INSERT INTO schema_migrations (version, applied_at) VALUES (18, ?1)",
                [crate::db::now_millis()],
            )
            .map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())?;
    }
    let members_migrated: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = 19)",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if !members_migrated {
        connection.execute_batch("BEGIN;
          CREATE TABLE shared_library_assets (asset_id TEXT PRIMARY KEY REFERENCES assets(id) ON DELETE CASCADE, library_id TEXT NOT NULL REFERENCES shared_libraries(id));
          CREATE INDEX shared_library_assets_library_idx ON shared_library_assets(library_id);
          INSERT INTO shared_library_assets SELECT a.id, l.id FROM assets a JOIN shared_libraries l ON coalesce(a.folder_reference, '') = l.source_key;
          DROP VIEW project_asset_access;
          CREATE VIEW project_asset_access AS SELECT project_id, id AS asset_id FROM assets UNION SELECT p.project_id, a.asset_id FROM project_libraries p JOIN shared_library_assets a ON a.library_id = p.library_id;
          INSERT INTO schema_migrations (version, applied_at) VALUES (19, 0);
          COMMIT;").map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command(async)]
pub fn list_shared_libraries(app: AppHandle) -> Result<Vec<SharedLibrary>, String> {
    let connection = open_connection(&app)?;
    let mut statement = connection.prepare("SELECT l.id, l.name, COUNT(a.asset_id) FROM shared_libraries l LEFT JOIN shared_library_assets a ON a.library_id = l.id AND a.asset_id IN (SELECT id FROM assets WHERE coalesce(json_extract(metadata_json, '$.libraryRemoved'), 0) = 0) GROUP BY l.id ORDER BY l.name COLLATE NOCASE, l.id").map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(SharedLibrary {
                id: row.get(0)?,
                name: row.get(1)?,
                asset_count: row.get(2)?,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_and_selection_share_assets_without_copying_or_expanding_legacy_projects() {
        let connection = Connection::open_in_memory().unwrap();
        crate::db::migrate(&connection).unwrap();
        connection.execute_batch("DELETE FROM schema_migrations WHERE version IN (18,19);
            DROP TABLE shared_library_assets;
            INSERT INTO projects (id,name,created_at,updated_at) VALUES ('old-a','A',0,0),('old-b','B',0,0),('new','New',0,0),('empty','Empty',0,0);
            INSERT INTO assets (id,project_id,kind,display_name,source_reference,folder_reference,analysis_status,metadata_json,created_at,updated_at) VALUES
              ('a','old-a','video','A','C:\\one\\a.mp4','C:\\one','ready','{}',0,0),
              ('b','old-b','video','B','C:\\two\\b.mp4','C:\\two','ready','{}',0,0);").unwrap();
        migrate(&connection).unwrap();
        let count = |project: &str| -> i64 {
            connection
                .query_row(
                    "SELECT COUNT(*) FROM project_asset_access WHERE project_id=?1",
                    [project],
                    |row| row.get(0),
                )
                .unwrap()
        };
        assert_eq!(count("old-a"), 1);
        assert_eq!(count("empty"), 0);
        connection.execute("INSERT INTO project_libraries SELECT 'new', id FROM shared_libraries WHERE source_key=?1", ["C:\\one"]).unwrap();
        assert_eq!(count("new"), 1);
        let id: String = connection
            .query_row(
                "SELECT asset_id FROM project_asset_access WHERE project_id='new'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(id, "a");
        connection
            .execute(
                "UPDATE assets SET folder_reference='D:\\moved' WHERE id='a'",
                [],
            )
            .unwrap();
        migrate(&connection).unwrap();
        assert_eq!(count("new"), 1);
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM assets", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            2
        );
        connection
            .execute("DELETE FROM project_libraries WHERE project_id='new'", [])
            .unwrap();
        assert_eq!(count("new"), 0);
        assert_eq!(count("old-a"), 1);
    }
}
