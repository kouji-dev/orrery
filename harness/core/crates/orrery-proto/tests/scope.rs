//! The subject string forms and the one-directional layer ordering.

mod scope {
    use orrery_proto::{
        AgentScope, BranchId, Consent, ExtId, Grant, Layer, Role, Subject, SubjectError,
    };

    #[test]
    fn subject_string_forms() {
        let cases = [
            (Subject::Agent, "agent"),
            (
                Subject::Ext(ExtId::new("buildgraph").unwrap()),
                "ext:buildgraph",
            ),
            (Subject::SubAgent("critic".into()), "agent:critic"),
        ];
        for (subject, wire) in cases {
            assert_eq!(subject.to_string(), wire);
            assert_eq!(
                serde_json::to_value(&subject).unwrap(),
                serde_json::json!(wire)
            );
            assert_eq!(
                serde_json::from_value::<Subject>(serde_json::json!(wire)).unwrap(),
                subject
            );
        }
        assert!("ext:".parse::<Subject>().is_err());
        assert!("agent:".parse::<Subject>().is_err());
        assert!("nobody".parse::<Subject>().is_err());
        assert!(
            "ext:Nope".parse::<Subject>().unwrap_err()
                == SubjectError {
                    value: "ext:Nope".into()
                }
        );
    }

    #[test]
    fn layer_ordering() {
        assert!(Layer::Project > Layer::Managed);
        assert!(Layer::Workspace > Layer::User);
        assert!(Layer::User > Layer::Org);
        assert_eq!(
            [Layer::Project, Layer::Managed, Layer::User]
                .into_iter()
                .max(),
            Some(Layer::Project),
            "max() must mean `closest layer wins`"
        );
        assert_eq!(
            serde_json::to_value(Layer::Workspace).unwrap(),
            serde_json::json!("workspace")
        );
    }

    #[test]
    fn role_wire_names() {
        for (role, wire) in [
            (Role::Planner, "planner"),
            (Role::Executor, "executor"),
            (Role::Verifier, "verifier"),
            (Role::Compactor, "compactor"),
            (Role::Summariser, "summariser"),
            (Role::Router, "router"),
            (Role::Grader, "grader"),
        ] {
            assert_eq!(serde_json::to_value(role).unwrap(), serde_json::json!(wire));
        }
    }

    #[test]
    fn agent_scope_round_trips() {
        let scope = AgentScope {
            agent: "critic".into(),
            branch: BranchId::new(),
            tools: vec!["builtin.read".into()],
            grant: Grant {
                capabilities: vec![],
                consent: Consent::Once,
            },
        };
        let json = serde_json::to_string(&scope).unwrap();
        assert_eq!(serde_json::from_str::<AgentScope>(&json).unwrap(), scope);
    }
}
