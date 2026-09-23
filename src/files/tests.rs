use super::*;

fn config() -> GuildConfig {
    GuildConfig {
        guild_id: 1,
        setup_completed: true,
        log_channel_id: Some(4),
        rule_source_channel_id: None,
        log_level: crate::logging::Level::Info,
        retention: crate::retention::Policy::None,
        channel_ids: vec![3, 5],
        admin_role_ids: vec![2, 6],
    }
}

#[test]
fn either_manager_role_grants_access_in_either_bot_channel() {
    let config = config();
    for role in [2, 6] {
        let mut member = Member::default();
        member.roles = vec![RoleId::new(99), RoleId::new(role)];
        for channel in [3, 5] {
            assert!(authorize(Some(&config), Some(&member), ChannelId::new(channel)).is_ok());
        }
    }
}

#[test]
fn unrelated_roles_and_moderation_permissions_do_not_grant_file_access() {
    let mut member = Member::default();
    member.roles = vec![RoleId::new(99)];
    for permissions in [
        None,
        Some(Permissions::empty()),
        Some(Permissions::MANAGE_MESSAGES | Permissions::MANAGE_ROLES),
    ] {
        member.permissions = permissions;
        assert!(authorize(Some(&config()), Some(&member), ChannelId::new(3)).is_err());
    }
}

#[test]
fn server_managers_retain_access_but_must_use_a_bot_channel() {
    let mut member = Member::default();
    for permissions in [Permissions::ADMINISTRATOR, Permissions::MANAGE_GUILD] {
        member.permissions = Some(permissions);
        assert!(authorize(Some(&config()), Some(&member), ChannelId::new(3)).is_ok());
        assert!(authorize(Some(&config()), Some(&member), ChannelId::new(4)).is_err());
    }
}

#[test]
fn missing_or_incomplete_setup_and_missing_member_deny_access() {
    let mut member = Member::default();
    member.permissions = Some(Permissions::ADMINISTRATOR);
    assert!(authorize(None, Some(&member), ChannelId::new(3)).is_err());
    assert!(authorize(Some(&config()), None, ChannelId::new(3)).is_err());
    let mut incomplete = config();
    incomplete.setup_completed = false;
    assert!(authorize(Some(&incomplete), Some(&member), ChannelId::new(3)).is_err());
}

#[test]
fn removing_a_manager_role_or_bot_channel_revokes_access() {
    let mut config = config();
    let mut member = Member::default();
    member.roles = vec![RoleId::new(6)];
    assert!(authorize(Some(&config), Some(&member), ChannelId::new(3)).is_ok());
    config.admin_role_ids = vec![2];
    assert!(authorize(Some(&config), Some(&member), ChannelId::new(3)).is_err());
    config.admin_role_ids = vec![6];
    config.channel_ids = vec![5];
    assert!(authorize(Some(&config), Some(&member), ChannelId::new(3)).is_err());
}
