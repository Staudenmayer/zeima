#![cfg(feature = "db")]

//! The iCalendar file of a meeting holds the properties the app writes and no others.

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::json;
use zeima_core::clock::ManualClock;
use zeima_core::sync::Versioned;
use zeima_core::{Core, MeetingInput, RemoteRow};

const T0: i64 = 1_791_187_200_000;
const HOUR: i64 = 3_600_000;
const WS: &str = "0192f000-0000-7000-8000-00000000a001";
const ORG: &str = "0192f000-0000-7000-8000-00000000b001";
const ADA: &str = "0192f000-0000-7000-8000-00000000c001";

async fn org() -> Core {
    let personal = common::with_activity(
        Core::open_in_memory(Arc::new(ManualClock::new(T0)))
            .await
            .unwrap(),
    )
    .await;
    personal
        .adopt_org_workspace(WS, ORG, "Acme", ADA, "Ada")
        .await
        .unwrap();
    personal.scoped(WS, ADA)
}

fn input() -> MeetingInput {
    MeetingInput {
        title: "Weekly sync".into(),
        start_at: T0 + HOUR,
        end_at: T0 + 2 * HOUR,
        tz: Some("UTC".into()),
        location: None,
        room_id: None,
        online_url: Some("https://meet.example.com/room".into()),
        project_id: None,
        activity_id: None,
        tag_ids: Vec::new(),
        billable: false,
        cost_center_id: None,
        note: String::new(),
        rrule: None,
        attendee_ids: Vec::new(),
    }
}

/// The property names of the unfolded lines of an .ics file.
fn properties(ics: &str) -> Vec<String> {
    ics.split("\r\n")
        .filter(|l| !l.is_empty())
        .map(|l| l.split([':', ';']).next().unwrap().to_string())
        .collect()
}

fn assert_only_known(ics: &str) {
    const KNOWN: [&str; 14] = [
        "BEGIN",
        "END",
        "VERSION",
        "PRODID",
        "UID",
        "DTSTAMP",
        "DTSTART",
        "DTEND",
        "SUMMARY",
        "RRULE",
        "LOCATION",
        "URL",
        "DESCRIPTION",
        "STATUS",
    ];
    for p in properties(ics) {
        assert!(
            KNOWN.contains(&p.as_str()),
            "unexpected property {p:?} in {ics:?}"
        );
    }
    assert!(!ics.contains('\r') || ics.matches('\r').count() == ics.matches("\r\n").count());
}

#[tokio::test]
async fn line_breaks_in_text_fields_do_not_start_new_properties() {
    let ada = org().await;
    let m = ada
        .create_meeting(MeetingInput {
            title: "Sync\r\nATTENDEE:mailto:x@example.com".into(),
            note: "Notes\rORGANIZER:mailto:y@example.com\r\nDESCRIPTION:two".into(),
            location: Some("Room\r\nATTACH:http://example.com/x".into()),
            ..input()
        })
        .await
        .unwrap();
    let ics = ada.meeting_ics(&m.id).await.unwrap();
    assert_only_known(&ics);
    assert_eq!(properties(&ics).iter().filter(|p| *p == "DESCRIPTION").count(), 1);
    assert!(ics.contains("URL:https://meet.example.com/room"), "{ics}");
    assert!(ics.contains("SUMMARY:Sync"), "{ics}");
}

#[tokio::test]
async fn an_online_address_must_be_a_clean_web_address() {
    let ada = org().await;
    for bad in [
        "https://meet.example.com/\r\nATTENDEE:mailto:x@example.com",
        "https://meet.example.com/\nX-A:b",
        "javascript:alert(1)",
        "http://",
        "ftp://example.com/file",
    ] {
        let err = ada
            .create_meeting(MeetingInput {
                online_url: Some(bad.into()),
                ..input()
            })
            .await;
        assert!(err.is_err(), "{bad:?} was accepted");
    }
}

#[tokio::test]
async fn pulled_meeting_values_cannot_add_properties() {
    let ada = org().await;
    let h = format!("{:015}-000000-server", T0);
    let fields: BTreeMap<String, Versioned> = [
        ("userId", json!(ADA)),
        ("title", json!("Pulled")),
        ("startAt", json!(T0 + HOUR)),
        ("endAt", json!(T0 + 2 * HOUR)),
        ("tz", json!("UTC")),
        (
            "onlineUrl",
            json!("https://a.example.com/\r\nATTENDEE:mailto:x@example.com"),
        ),
        ("rrule", json!("FREQ=DAILY\r\nATTENDEE:mailto:x@example.com")),
        ("note", json!("")),
        ("attendeeIds", json!([])),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), Versioned { v, h: h.clone() }))
    .collect();
    let id = "0192f000-0000-7000-8000-0000000000d1";
    ada.sync_apply(&[RemoteRow {
        workspace_id: WS.into(),
        table: "meeting".into(),
        row_id: id.into(),
        fields,
        deleted: None,
    }])
    .await
    .unwrap();
    let ics = ada.meeting_ics(id).await.unwrap();
    assert_only_known(&ics);
    assert!(!ics.contains("ATTENDEE"), "{ics}");
    assert!(!ics.contains("RRULE"), "{ics}");
}

#[tokio::test]
async fn a_pulled_meeting_id_cannot_add_properties_through_the_uid() {
    let ada = org().await;
    let h = format!("{:015}-000000-server", T0);
    let fields: BTreeMap<String, Versioned> = [
        ("userId", json!(ADA)),
        ("title", json!("Pulled")),
        ("startAt", json!(T0 + HOUR)),
        ("endAt", json!(T0 + 2 * HOUR)),
        ("tz", json!("UTC")),
        ("note", json!("")),
        ("attendeeIds", json!([])),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), Versioned { v, h: h.clone() }))
    .collect();
    let id = "0192f000-0000-7000-8000-0000000000d2\r\nATTENDEE:mailto:x@example.com";
    ada.sync_apply(&[RemoteRow {
        workspace_id: WS.into(),
        table: "meeting".into(),
        row_id: id.into(),
        fields,
        deleted: None,
    }])
    .await
    .unwrap();
    let ics = ada.meeting_ics(id).await.unwrap();
    assert_only_known(&ics);
    assert!(!ics.contains("ATTENDEE"), "{ics}");
}
