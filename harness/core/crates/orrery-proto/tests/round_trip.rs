//! Round-trip tests for the wire types.

mod ids {
    use orrery_proto::{Seq, TurnId};
    use std::str::FromStr;

    #[test]
    fn round_trips_as_plain_string() {
        let id = TurnId::new();
        let json = serde_json::to_value(id).unwrap();
        assert!(json.is_string(), "a TurnId must serialise to a bare string");
        assert_eq!(json.as_str().unwrap(), id.to_string());

        let back: TurnId = serde_json::from_value(json).unwrap();
        assert_eq!(back, id);

        assert_eq!(TurnId::from_str(&id.to_string()).unwrap(), id);
        assert!(TurnId::from_str("not-a-uuid").is_err());
        assert!(serde_json::from_str::<TurnId>("\"not-a-uuid\"").is_err());
    }

    #[test]
    fn seq_is_ordered() {
        assert!(Seq(1) < Seq(2));
        assert_eq!(serde_json::to_value(Seq(7)).unwrap(), serde_json::json!(7));
    }
}

mod ext_id {
    use orrery_proto::ExtId;
    use std::str::FromStr;

    #[test]
    fn accepts_plain_and_mcp_namespaces() {
        for ok in ["buildgraph", "builtin", "ripgrep-search", "mcp.jira", "a1"] {
            ExtId::new(ok).unwrap_or_else(|e| panic!("{ok} should be valid: {e}"));
        }
        assert_eq!(ExtId::from_str("mcp.jira").unwrap().as_str(), "mcp.jira");
        assert_eq!(
            serde_json::to_value(ExtId::new("buildgraph").unwrap()).unwrap(),
            serde_json::json!("buildgraph")
        );
    }

    #[test]
    fn rejects_bad_namespaces() {
        for bad in [
            "",
            "Buildgraph",
            "build_graph",
            "a.b",
            "mcp.",
            "mcp.jira.x",
            ".mcp",
        ] {
            assert!(ExtId::new(bad).is_err(), "{bad:?} should be rejected");
        }
        assert!(serde_json::from_str::<ExtId>("\"Nope\"").is_err());
    }
}

mod session_ref {
    use orrery_proto::{BranchId, SessionId, SessionRef};

    #[test]
    fn round_trips() {
        let r = SessionRef {
            session: SessionId::new(),
            branch: BranchId::new(),
            turn: None,
        };
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<SessionRef>(&json).unwrap(), r);
    }
}
