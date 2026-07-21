//! ZITADEL human user 创建请求的共享构造逻辑。

use crate::{
    CreateHumanIdentity,
    generated::zitadel::user::v2::{
        CreateUserRequest, Password, SetHumanEmail, SetHumanPhone, SetHumanProfile,
        create_user_request::Human as CreateHumanUser,
    },
};

/// ZITADEL 创建 human user 请求的只读检查快照。
///
/// 该类型仅用于框架集成测试验证生成的 UserService v2 请求语义，不作为业务应用的稳定契约。
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZitadelCreateHumanUserRequestInspection {
    /// 请求目标 ZITADEL Organization ID。
    pub organization_id: String,
    /// 传给 ZITADEL 的登录用户名。
    pub username: String,
    /// human email 联系邮箱。
    pub email: String,
    /// email 是否以已验证状态写入。
    pub email_is_verified: bool,
    /// email 是否请求 ZITADEL 发送验证码。
    pub email_send_code: bool,
    /// human phone/mobile 联系手机号。
    pub contact_phone: Option<String>,
    /// phone 是否以已验证状态写入。
    pub phone_is_verified: Option<bool>,
    /// phone 是否请求 ZITADEL 发送验证码。
    pub phone_send_code: bool,
}

/// 构造并检查 ZITADEL `CreateUserRequest` 的 human 联系字段。
///
/// 该函数服务于集成测试：它先调用生产代码共享的 protobuf 构造函数，再从生成的 request
/// 中读取关键字段，确保测试覆盖的是实际发送给 ZITADEL 的 oneof 语义。
#[doc(hidden)]
pub fn inspect_create_human_user_request(
    organization_id: &str,
    request: &CreateHumanIdentity,
    contact_phone: Option<&str>,
) -> ZitadelCreateHumanUserRequestInspection {
    let create = create_human_user_request(organization_id, request, contact_phone);
    let human = create
        .human_opt()
        .into_option()
        .expect("ZITADEL human create request must contain human payload");
    let email = human
        .email_opt()
        .into_option()
        .expect("ZITADEL human create request must contain email payload");
    let phone = human.phone_opt().into_option();

    ZitadelCreateHumanUserRequestInspection {
        organization_id: create
            .organization_id()
            .to_str()
            .expect("organization ID should be valid UTF-8")
            .to_owned(),
        username: create
            .username()
            .to_str()
            .expect("username should be valid UTF-8")
            .to_owned(),
        email: email
            .email()
            .to_str()
            .expect("email should be valid UTF-8")
            .to_owned(),
        email_is_verified: email.is_verified(),
        email_send_code: email.send_code_opt().into_option().is_some(),
        contact_phone: phone.as_ref().map(|phone| {
            phone
                .phone()
                .to_str()
                .expect("phone should be valid UTF-8")
                .to_owned()
        }),
        phone_is_verified: phone.as_ref().map(|phone| phone.is_verified()),
        phone_send_code: phone
            .as_ref()
            .is_some_and(|phone| phone.send_code_opt().into_option().is_some()),
    }
}

pub(crate) fn create_human_user_request(
    organization_id: &str,
    request: &CreateHumanIdentity,
    contact_phone: Option<&str>,
) -> CreateUserRequest {
    let mut profile = SetHumanProfile::new();
    profile.set_given_name(request.given_name.as_str());
    profile.set_family_name(request.family_name.as_str());
    if let Some(display_name) = request.display_name.as_deref() {
        profile.set_display_name(display_name);
    }

    let mut email = SetHumanEmail::new();
    email.set_email(request.email.as_str());
    email.set_is_verified(true);

    let mut human = CreateHumanUser::new();
    human.set_profile(profile);
    human.set_email(email);
    if let Some(contact_phone) = contact_phone
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let mut phone = SetHumanPhone::new();
        phone.set_phone(contact_phone);
        phone.set_is_verified(true);
        human.set_phone(phone);
    }

    let mut password = Password::new();
    password.set_password(request.initial_password.as_str());
    password.set_change_required(request.require_password_change);
    human.set_password(password);

    let mut create = CreateUserRequest::new();
    create.set_organization_id(organization_id);
    create.set_username(request.username.as_str());
    create.set_human(human);
    create
}
