//! Truth-table unit tests for the merge engine. Each test builds a precise local
//! state + a single remote row (with hand-chosen HLCs) and asserts the result.
//! HLCs are zero-padded so lexicographic order == numeric order.

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::json;

use super::merge_bucket;
use crate::services::cloud_sync::pipeline::buckets::Bucket;
use crate::services::cloud_sync::pipeline::rows::ParsedRow;

fn mem() -> Connection {
  let conn = Connection::open_in_memory().unwrap();
  conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
  for sql in crate::db::schema::get_migrations() {
    conn.execute_batch(sql).unwrap();
  }
  conn
}

fn parsed(value: serde_json::Value) -> ParsedRow {
  let hlc = value
    .get("_hlc")
    .and_then(|v| v.as_str())
    .unwrap_or("")
    .to_string();
  let deleted = value
    .get("_deleted")
    .and_then(|v| v.as_bool())
    .unwrap_or(false);
  ParsedRow {
    value,
    hlc,
    deleted,
  }
}

// --- tag_categories (a simple single-PK entity) ---------------------------

fn insert_tag_category(conn: &Connection, id: &str, name: &str, hlc: &str) {
  conn
    .execute(
      "INSERT INTO tag_categories (id, name, sort_order, color, _hlc) VALUES (?1, ?2, 0, NULL, ?3)",
      params![id, name, hlc],
    )
    .unwrap();
}

fn tc_live(id: &str, name: &str, hlc: &str) -> ParsedRow {
  parsed(
    json!({"id": id, "name": name, "color": null, "sort_order": 0, "_hlc": hlc, "_deleted": false}),
  )
}

fn tc_tomb(id: &str, hlc: &str) -> ParsedRow {
  parsed(json!({"id": id, "_hlc": hlc, "_deleted": true}))
}

fn tc_name(conn: &Connection, id: &str) -> Option<String> {
  conn
    .query_row("SELECT name FROM tag_categories WHERE id = ?1", [id], |r| {
      r.get(0)
    })
    .optional()
    .unwrap()
}

fn tomb_hlc(conn: &Connection, etype: &str, eid: &str) -> Option<String> {
  conn
    .query_row(
      "SELECT _hlc FROM sync_tombstones WHERE entity_type = ?1 AND entity_id = ?2",
      params![etype, eid],
      |r| r.get(0),
    )
    .optional()
    .unwrap()
}

#[test]
fn entity_insert_when_absent() {
  let conn = mem();
  merge_bucket(
    &conn,
    &Bucket::TagCategories,
    &[tc_live("c1", "House", "0005")],
  )
  .unwrap();
  assert_eq!(tc_name(&conn, "c1").as_deref(), Some("House"));
}

#[test]
fn entity_replace_when_remote_newer() {
  let conn = mem();
  insert_tag_category(&conn, "c1", "House", "0005");
  merge_bucket(
    &conn,
    &Bucket::TagCategories,
    &[tc_live("c1", "Techno", "0009")],
  )
  .unwrap();
  assert_eq!(tc_name(&conn, "c1").as_deref(), Some("Techno"));
}

#[test]
fn entity_keep_when_remote_older() {
  let conn = mem();
  insert_tag_category(&conn, "c1", "House", "0009");
  merge_bucket(
    &conn,
    &Bucket::TagCategories,
    &[tc_live("c1", "Techno", "0005")],
  )
  .unwrap();
  assert_eq!(tc_name(&conn, "c1").as_deref(), Some("House"));
}

#[test]
fn entity_noop_when_equal_hlc() {
  let conn = mem();
  insert_tag_category(&conn, "c1", "House", "0005");
  merge_bucket(
    &conn,
    &Bucket::TagCategories,
    &[tc_live("c1", "House", "0005")],
  )
  .unwrap();
  assert_eq!(tc_name(&conn, "c1").as_deref(), Some("House"));
}

#[test]
fn entity_delete_when_remote_newer() {
  let conn = mem();
  insert_tag_category(&conn, "c1", "House", "0005");
  merge_bucket(&conn, &Bucket::TagCategories, &[tc_tomb("c1", "0009")]).unwrap();
  assert_eq!(tc_name(&conn, "c1"), None, "row deleted");
  assert_eq!(
    tomb_hlc(&conn, "tag_categories", "c1").as_deref(),
    Some("0009")
  );
}

#[test]
fn entity_keep_when_delete_older() {
  let conn = mem();
  insert_tag_category(&conn, "c1", "House", "0009");
  merge_bucket(&conn, &Bucket::TagCategories, &[tc_tomb("c1", "0005")]).unwrap();
  assert_eq!(
    tc_name(&conn, "c1").as_deref(),
    Some("House"),
    "kept (local newer)"
  );
  assert_eq!(tomb_hlc(&conn, "tag_categories", "c1"), None);
}

#[test]
fn entity_delete_wins_tie() {
  let conn = mem();
  insert_tag_category(&conn, "c1", "House", "0007");
  merge_bucket(&conn, &Bucket::TagCategories, &[tc_tomb("c1", "0007")]).unwrap();
  assert_eq!(tc_name(&conn, "c1"), None, "delete wins the tie");
}

#[test]
fn entity_resurrect_when_update_newer_than_tombstone() {
  let conn = mem();
  conn.execute(
        "INSERT INTO sync_tombstones (entity_type, entity_id, _hlc) VALUES ('tag_categories','c1','0010')",
        [],
    )
    .unwrap();
  merge_bucket(
    &conn,
    &Bucket::TagCategories,
    &[tc_live("c1", "House", "0020")],
  )
  .unwrap();
  assert_eq!(
    tc_name(&conn, "c1").as_deref(),
    Some("House"),
    "resurrected"
  );
  assert_eq!(
    tomb_hlc(&conn, "tag_categories", "c1"),
    None,
    "tombstone dropped"
  );
}

#[test]
fn entity_stays_deleted_when_update_older_than_tombstone() {
  let conn = mem();
  conn.execute(
        "INSERT INTO sync_tombstones (entity_type, entity_id, _hlc) VALUES ('tag_categories','c1','0020')",
        [],
    )
    .unwrap();
  merge_bucket(
    &conn,
    &Bucket::TagCategories,
    &[tc_live("c1", "House", "0010")],
  )
  .unwrap();
  assert_eq!(tc_name(&conn, "c1"), None, "stays deleted");
}

#[test]
fn entity_records_tombstone_when_never_seen() {
  let conn = mem();
  merge_bucket(&conn, &Bucket::TagCategories, &[tc_tomb("c1", "0009")]).unwrap();
  assert_eq!(
    tomb_hlc(&conn, "tag_categories", "c1").as_deref(),
    Some("0009")
  );
}

#[test]
fn entity_merge_is_idempotent() {
  let conn = mem();
  let rows = [tc_live("c1", "House", "0005"), tc_tomb("c2", "0006")];
  merge_bucket(&conn, &Bucket::TagCategories, &rows).unwrap();
  let after_once = (
    tc_name(&conn, "c1"),
    tomb_hlc(&conn, "tag_categories", "c2"),
  );
  merge_bucket(&conn, &Bucket::TagCategories, &rows).unwrap();
  let after_twice = (
    tc_name(&conn, "c1"),
    tomb_hlc(&conn, "tag_categories", "c2"),
  );
  assert_eq!(after_once, after_twice);
}

#[test]
fn entity_unique_collision_is_skipped_not_fatal() {
  let conn = mem();
  // Local "House" with id c-local; remote "House" with a different id c-remote.
  insert_tag_category(&conn, "c-local", "House", "0005");
  let res = merge_bucket(
    &conn,
    &Bucket::TagCategories,
    &[
      tc_live("c-remote", "House", "0009"), // UNIQUE(name) collision -> skipped
      tc_live("c-other", "Techno", "0009"), // still merges
    ],
  );
  assert!(res.is_ok(), "bucket must not abort on a UNIQUE collision");
  assert_eq!(tc_name(&conn, "c-local").as_deref(), Some("House"));
  assert_eq!(tc_name(&conn, "c-remote"), None, "duplicate skipped");
  assert_eq!(tc_name(&conn, "c-other").as_deref(), Some("Techno"));
}

// --- track_tags (a tag junction: ADD-WINS-TIE, no ordering) ----------------

fn setup_junction_endpoints(conn: &Connection) {
  conn
    .execute(
      "INSERT INTO tag_categories (id, name, sort_order, _hlc) VALUES ('c1','Cat',0,'0001')",
      [],
    )
    .unwrap();
  conn.execute(
        "INSERT INTO tags (id, category_id, name, sort_order, _hlc) VALUES ('g1','c1','Tag',0,'0001')",
        [],
    )
    .unwrap();
  conn
    .execute(
      "INSERT INTO tracks (id, file_path, duration_ms, date_added, date_modified, _hlc) \
         VALUES ('t1','/x',1000,'2020-01-01T00:00:00Z','2020-01-01T00:00:00Z','0001')",
      [],
    )
    .unwrap();
}

fn tt_live(hlc: &str) -> ParsedRow {
  parsed(json!({"track_id": "t1", "tag_id": "g1", "_hlc": hlc, "_deleted": false}))
}

fn tt_tomb(hlc: &str) -> ParsedRow {
  parsed(json!({"track_id": "t1", "tag_id": "g1", "_hlc": hlc, "_deleted": true}))
}

fn tt_exists(conn: &Connection) -> bool {
  conn
    .query_row(
      "SELECT 1 FROM track_tags WHERE track_id='t1' AND tag_id='g1'",
      [],
      |_| Ok(()),
    )
    .optional()
    .unwrap()
    .is_some()
}

#[test]
fn junction_insert_when_endpoints_exist() {
  let conn = mem();
  setup_junction_endpoints(&conn);
  merge_bucket(&conn, &Bucket::TrackTags, &[tt_live("0005")]).unwrap();
  assert!(tt_exists(&conn));
}

#[test]
fn junction_orphan_insert_is_skipped() {
  let conn = mem();
  // No endpoints inserted -> the add references missing track/tag.
  merge_bucket(&conn, &Bucket::TrackTags, &[tt_live("0005")]).unwrap();
  assert!(!tt_exists(&conn), "orphan membership skipped");
}

#[test]
fn junction_add_wins_over_older_delete() {
  let conn = mem();
  setup_junction_endpoints(&conn);
  conn.execute(
        "INSERT INTO sync_tombstones (entity_type, entity_id, _hlc) VALUES ('track_tags','t1|g1','0010')",
        [],
    )
    .unwrap();
  merge_bucket(&conn, &Bucket::TrackTags, &[tt_live("0020")]).unwrap();
  assert!(tt_exists(&conn), "add newer than removal resurrects");
}

#[test]
fn junction_delete_wins_only_when_strictly_newer() {
  let conn = mem();
  setup_junction_endpoints(&conn);
  conn
    .execute(
      "INSERT INTO track_tags (track_id, tag_id, _hlc) VALUES ('t1','g1','0010')",
      [],
    )
    .unwrap();
  // delete older -> keep
  merge_bucket(&conn, &Bucket::TrackTags, &[tt_tomb("0005")]).unwrap();
  assert!(tt_exists(&conn), "older delete keeps the add");
  // delete newer -> remove
  merge_bucket(&conn, &Bucket::TrackTags, &[tt_tomb("0020")]).unwrap();
  assert!(!tt_exists(&conn), "newer delete removes the add");
}

#[test]
fn junction_delete_tie_keeps_add() {
  let conn = mem();
  setup_junction_endpoints(&conn);
  conn
    .execute(
      "INSERT INTO track_tags (track_id, tag_id, _hlc) VALUES ('t1','g1','0010')",
      [],
    )
    .unwrap();
  merge_bucket(&conn, &Bucket::TrackTags, &[tt_tomb("0010")]).unwrap();
  assert!(tt_exists(&conn), "add wins the tie");
}

// --- settings (LWW per whitelisted key) -----------------------------------

fn setting_value(conn: &Connection, key: &str) -> Option<String> {
  conn
    .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
      r.get(0)
    })
    .optional()
    .unwrap()
}

#[test]
fn settings_apply_whitelisted_newer() {
  let conn = mem();
  merge_bucket(
    &conn,
    &Bucket::Settings,
    &[parsed(
      json!({"key": "theme", "value": "dark", "_hlc": "0005"}),
    )],
  )
  .unwrap();
  assert_eq!(setting_value(&conn, "theme").as_deref(), Some("dark"));
  let hlc: String = conn
    .query_row(
      "SELECT value FROM sync_state WHERE key = 'setting_hlc:theme'",
      [],
      |r| r.get(0),
    )
    .unwrap();
  assert_eq!(hlc, "0005");
}

#[test]
fn settings_drop_non_whitelisted() {
  let conn = mem();
  merge_bucket(
    &conn,
    &Bucket::Settings,
    &[parsed(
      json!({"key": "audio_device", "value": "X", "_hlc": "0009"}),
    )],
  )
  .unwrap();
  assert_eq!(
    setting_value(&conn, "audio_device"),
    None,
    "device-local key dropped"
  );
}

#[test]
fn settings_keep_local_when_older() {
  let conn = mem();
  conn
    .execute(
      "INSERT INTO settings (key, value) VALUES ('theme','dark')",
      [],
    )
    .unwrap();
  conn
    .execute(
      "INSERT INTO sync_state (key, value) VALUES ('setting_hlc:theme','0009')",
      [],
    )
    .unwrap();
  merge_bucket(
    &conn,
    &Bucket::Settings,
    &[parsed(
      json!({"key": "theme", "value": "light", "_hlc": "0005"}),
    )],
  )
  .unwrap();
  assert_eq!(
    setting_value(&conn, "theme").as_deref(),
    Some("dark"),
    "older remote ignored"
  );
}

// --- override reporting (observational — must not change merge outcomes) ---

fn set_node_id(conn: &Connection, node: &str) {
  conn
    .execute(
      "INSERT INTO sync_state (key, value) VALUES ('node_id', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
      [node],
    )
    .unwrap();
}

#[test]
fn override_reported_when_local_authored_value_is_replaced() {
  let conn = mem();
  set_node_id(&conn, "aabbccdd");
  // Local "House" authored by THIS device (node aabbccdd).
  insert_tag_category(&conn, "c1", "House", "0000000000000005-00000000-aabbccdd");
  // Remote rename with a higher HLC from ANOTHER device.
  let overrides = merge_bucket(
    &conn,
    &Bucket::TagCategories,
    &[tc_live(
      "c1",
      "Techno",
      "0000000000000006-00000000-11223344",
    )],
  )
  .unwrap();
  // Outcome is unchanged (remote wins); the override is reported alongside.
  assert_eq!(tc_name(&conn, "c1").as_deref(), Some("Techno"));
  assert_eq!(overrides.len(), 1);
  assert_eq!(
    overrides[0].label, "House",
    "names the discarded local value"
  );
}

#[test]
fn no_override_when_local_value_authored_elsewhere() {
  let conn = mem();
  set_node_id(&conn, "aabbccdd");
  // Local value was authored by a DIFFERENT device (node 99999999) — plain
  // propagation, not a conflict this device should be toasted about.
  insert_tag_category(&conn, "c1", "House", "0000000000000005-00000000-99999999");
  let overrides = merge_bucket(
    &conn,
    &Bucket::TagCategories,
    &[tc_live(
      "c1",
      "Techno",
      "0000000000000006-00000000-11223344",
    )],
  )
  .unwrap();
  assert_eq!(tc_name(&conn, "c1").as_deref(), Some("Techno"));
  assert!(overrides.is_empty());
}

// --- cues (anti-regression: the `upsert_cue` stray-column defect) ----------

fn seed_cue_track(conn: &Connection) {
  conn
    .execute(
      "INSERT INTO tracks (id, file_path, duration_ms, date_added, date_modified, url, _hlc) \
       VALUES ('t1','/x',1000,'2020-01-01T00:00:00Z','2020-01-01T00:00:00Z','https://t1.test','0001')",
      [],
    )
    .unwrap();
}

fn cue_live(hlc: &str, position_ms: i64, name: &str) -> ParsedRow {
  parsed(json!({
    "id": "cue-1", "track_id": "t1", "position_ms": position_ms, "cue_type": "hot",
    "loop_end_ms": null, "hot_cue_index": 3, "name": name, "color": "#ff0000",
    "_hlc": hlc, "_deleted": false,
  }))
}

fn cue_tomb(hlc: &str) -> ParsedRow {
  parsed(json!({"id": "cue-1", "_hlc": hlc, "_deleted": true}))
}

/// `(track_id, position_ms, type, loop_end_ms, hot_cue_index, name, color)` of `cue-1`.
type CueState = (
  String,
  i64,
  String,
  Option<i64>,
  Option<i32>,
  Option<String>,
  Option<String>,
);

fn cue_state(conn: &Connection) -> Option<CueState> {
  conn
    .query_row(
      "SELECT track_id, position_ms, type, loop_end_ms, hot_cue_index, name, color \
       FROM cues WHERE id = 'cue-1'",
      [],
      |r| {
        Ok((
          r.get(0)?,
          r.get(1)?,
          r.get(2)?,
          r.get(3)?,
          r.get(4)?,
          r.get(5)?,
          r.get(6)?,
        ))
      },
    )
    .optional()
    .unwrap()
}

/// Live + update + tombstone through `merge_bucket` so the cue UPSERT SQL is really
/// prepared and executed against the migrated schema. If a column from another table
/// (the original bug: `url=excluded.url`) ever leaks into that statement, SQLite
/// rejects it at PREPARE time and this test fails — instead of the app failing at
/// runtime with 296 green tests.
#[test]
fn cue_merge_insert_update_delete_exercises_the_cue_upsert() {
  let conn = mem();
  seed_cue_track(&conn);

  merge_bucket(&conn, &Bucket::Cues, &[cue_live("0005", 12345, "Drop")]).unwrap();
  assert_eq!(
    cue_state(&conn),
    Some((
      "t1".to_string(),
      12345,
      "hot".to_string(),
      None,
      Some(3),
      Some("Drop".to_string()),
      Some("#ff0000".to_string()),
    )),
    "live cue row must land with every column in its own field"
  );

  // Conflict path: the ON CONFLICT DO UPDATE SET clause itself, where the stray column lived.
  merge_bucket(&conn, &Bucket::Cues, &[cue_live("0009", 54321, "Break")]).unwrap();
  assert_eq!(
    cue_state(&conn),
    Some((
      "t1".to_string(),
      54321,
      "hot".to_string(),
      None,
      Some(3),
      Some("Break".to_string()),
      Some("#ff0000".to_string()),
    )),
    "newer remote cue must overwrite via the UPSERT"
  );

  // HLC guard intact: an older remote row must not clobber the newer local one.
  merge_bucket(&conn, &Bucket::Cues, &[cue_live("0007", 111, "Stale")]).unwrap();
  assert_eq!(cue_state(&conn).unwrap().1, 54321, "older remote ignored");

  merge_bucket(&conn, &Bucket::Cues, &[cue_tomb("0020")]).unwrap();
  assert_eq!(cue_state(&conn), None, "tombstone deletes the cue");
  assert_eq!(
    tomb_hlc(&conn, "cues", "cue-1").as_deref(),
    Some("0020"),
    "tombstone recorded"
  );

  // Synced-columns invariant: the cue UPSERT never touches the tracks row it references.
  let url: Option<String> = conn
    .query_row("SELECT url FROM tracks WHERE id = 't1'", [], |r| r.get(0))
    .unwrap();
  assert_eq!(
    url.as_deref(),
    Some("https://t1.test"),
    "track row untouched"
  );
}
