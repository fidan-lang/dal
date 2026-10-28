use anyhow::Context;
use aws_sdk_cognitoidentityprovider::{
    Client,
    types::{
        AuthFlowType, ChallengeNameType, SoftwareTokenMfaSettingsType,
        VerifySoftwareTokenResponseType,
    },
};
use dal_common::error::DalError;
use std::fmt::{Debug, Display};
use tracing::{error, warn};

/// Thin wrapper around the AWS Cognito Identity Provider SDK client.
pub struct CognitoClient {
    inner: Client,
    pool_id: String,
    client_id: String,
}

// Authentication-bearing values deliberately do not implement Debug or Serialize.
pub struct AuthTokens {
    pub access: String,
    pub id: String,
    pub refresh: String,
}
pub enum Authentication {
    Complete(AuthTokens),
    Totp { username: String, session: String },
}

fn authentication(
    result: Option<&aws_sdk_cognitoidentityprovider::types::AuthenticationResultType>,
    challenge: Option<&ChallengeNameType>,
    session: Option<&str>,
    username: Option<&str>,
) -> Result<Authentication, DalError> {
    match (result, challenge) {
        (Some(value), None) => {
            let required = |value: Option<&str>| {
                value
                    .filter(|v| {
                        !v.is_empty()
                            && v.len() <= 16384
                            && v.bytes()
                                .all(|c| c.is_ascii_alphanumeric() || b"-_.+/=".contains(&c))
                    })
                    .map(str::to_owned)
                    .ok_or(DalError::Unauthorized)
            };
            Ok(Authentication::Complete(AuthTokens {
                access: required(value.access_token())?,
                id: required(value.id_token())?,
                refresh: required(value.refresh_token())?,
            }))
        }
        (None, Some(ChallengeNameType::SoftwareTokenMfa)) => Ok(Authentication::Totp {
            username: username
                .filter(|v| !v.is_empty())
                .ok_or(DalError::Unauthorized)?
                .to_owned(),
            session: session
                .filter(|v| !v.is_empty() && v.len() <= 4096)
                .ok_or(DalError::Unauthorized)?
                .to_owned(),
        }),
        // Never treat unsupported/setup/SMS challenges, mixed responses or empty tokens as login.
        _ => Err(DalError::Unauthorized),
    }
}

impl CognitoClient {
    /// Build from a pre-loaded `SdkConfig` (shared with other AWS clients).
    /// Pass `endpoint_url` to override the Cognito endpoint — used for local
    /// dev with `cognito-local` or LocalStack.
    pub fn from_sdk_config(
        aws_cfg: &aws_config::SdkConfig,
        pool_id: String,
        client_id: String,
        endpoint_url: Option<&str>,
    ) -> Self {
        let client_cfg = match endpoint_url {
            Some(url) => aws_sdk_cognitoidentityprovider::config::Builder::from(aws_cfg)
                .endpoint_url(url)
                .build(),
            None => aws_sdk_cognitoidentityprovider::config::Builder::from(aws_cfg).build(),
        };
        let inner = Client::from_conf(client_cfg);
        Self {
            inner,
            pool_id,
            client_id,
        }
    }

    /// Sign in with username (or email) + password.
    /// Returns complete tokens or a supported software-token challenge.
    pub async fn begin_sign_in(
        &self,
        username: &str,
        password: &str,
    ) -> Result<Authentication, DalError> {
        let resp = self
            .inner
            .initiate_auth()
            .auth_flow(AuthFlowType::UserPasswordAuth)
            .client_id(&self.client_id)
            .auth_parameters("USERNAME", username)
            .auth_parameters("PASSWORD", password)
            .send()
            .await
            .map_err(|e| {
                if e.as_service_error().is_some_and(|error| {
                    error.is_not_authorized_exception() || error.is_user_not_found_exception()
                }) {
                    DalError::Unauthorized
                } else {
                    self.cognito_request_error("initiate_auth:user_password", Some(username), e)
                }
            })?;

        authentication(
            resp.authentication_result(),
            resp.challenge_name(),
            resp.session(),
            resp.challenge_parameters()
                .and_then(|p| p.get("USERNAME"))
                .map(String::as_str)
                .or(Some(username)),
        )
    }

    /// Token-only callers fail closed when the account needs MFA.
    pub async fn sign_in(
        &self,
        username: &str,
        password: &str,
    ) -> Result<(String, String, String), DalError> {
        match self.begin_sign_in(username, password).await? {
            Authentication::Complete(t) => Ok((t.access, t.id, t.refresh)),
            Authentication::Totp { .. } => Err(DalError::Unauthorized),
        }
    }

    pub async fn complete_totp(
        &self,
        username: &str,
        session: &str,
        code: &str,
    ) -> Result<AuthTokens, DalError> {
        if code.len() != 6 || !code.bytes().all(|c| c.is_ascii_digit()) {
            return Err(DalError::Unauthorized);
        }
        let response = self
            .inner
            .respond_to_auth_challenge()
            .client_id(&self.client_id)
            .challenge_name(ChallengeNameType::SoftwareTokenMfa)
            .session(session)
            .challenge_responses("USERNAME", username)
            .challenge_responses("SOFTWARE_TOKEN_MFA_CODE", code)
            .send()
            .await
            .map_err(|_| DalError::Unauthorized)?;
        match authentication(
            response.authentication_result(),
            response.challenge_name(),
            response.session(),
            Some(username),
        )? {
            Authentication::Complete(tokens) => Ok(tokens),
            _ => Err(DalError::Unauthorized),
        }
    }

    pub async fn totp_enabled(&self, username: &str) -> Result<bool, DalError> {
        let response = self
            .inner
            .admin_get_user()
            .user_pool_id(&self.pool_id)
            .username(username)
            .send()
            .await
            .map_err(|e| self.cognito_request_error("admin_get_user:mfa", None, e))?;
        Ok(response
            .user_mfa_setting_list()
            .iter()
            .any(|method| method == "SOFTWARE_TOKEN_MFA"))
    }

    pub async fn associate_totp(&self, access: &str) -> Result<String, DalError> {
        let response = self
            .inner
            .associate_software_token()
            .access_token(access)
            .send()
            .await
            .map_err(|e| self.cognito_request_error("associate_software_token", None, e))?;
        response
            .secret_code()
            .filter(|secret| {
                !secret.is_empty()
                    && secret.len() <= 128
                    && secret.bytes().all(|c| c.is_ascii_alphanumeric())
            })
            .map(str::to_owned)
            .ok_or(DalError::Unauthorized)
    }

    /// Only a definite Cognito code rejection can roll back enrollment intent.
    /// Transport/provider ambiguity leaves the durable downgrade guard enabled.
    pub async fn verify_totp(&self, access: &str, code: &str) -> Result<bool, DalError> {
        if code.len() != 6 || !code.bytes().all(|c| c.is_ascii_digit()) {
            return Ok(false);
        }
        let response = match self
            .inner
            .verify_software_token()
            .access_token(access)
            .user_code(code)
            .send()
            .await
        {
            Ok(response) => response,
            Err(error)
                if error
                    .as_service_error()
                    .is_some_and(|e| e.is_code_mismatch_exception()) =>
            {
                return Ok(false);
            }
            Err(error) => {
                return Err(self.cognito_request_error("verify_software_token", None, error));
            }
        };
        match response.status() {
            Some(VerifySoftwareTokenResponseType::Success) => Ok(true),
            Some(VerifySoftwareTokenResponseType::Error) => Ok(false),
            _ => Err(DalError::Cognito(
                "invalid verify_software_token response".into(),
            )),
        }
    }

    pub async fn set_totp(&self, access: &str, enabled: bool) -> Result<(), DalError> {
        self.inner
            .set_user_mfa_preference()
            .access_token(access)
            .software_token_mfa_settings(
                SoftwareTokenMfaSettingsType::builder()
                    .enabled(enabled)
                    .preferred_mfa(enabled)
                    .build(),
            )
            .send()
            .await
            .map_err(|e| self.cognito_request_error("set_user_mfa_preference", None, e))?;
        Ok(())
    }

    /// Refresh tokens using a refresh_token.
    pub async fn refresh(&self, refresh_token: &str) -> Result<(String, String), DalError> {
        let resp = self
            .inner
            .initiate_auth()
            .auth_flow(AuthFlowType::RefreshTokenAuth)
            .client_id(&self.client_id)
            .auth_parameters("REFRESH_TOKEN", refresh_token)
            .send()
            .await
            .map_err(|e| self.cognito_request_error("initiate_auth:refresh_token", None, e))?;

        let result = resp.authentication_result().ok_or_else(|| {
            warn!(
                operation = "initiate_auth:refresh_token",
                "cognito returned no authentication result"
            );
            DalError::Cognito(
                "cognito initiate_auth:refresh_token returned no authentication result".into(),
            )
        })?;

        let access = result.access_token().unwrap_or_default().to_string();
        let id = result.id_token().unwrap_or_default().to_string();
        Ok((access, id))
    }

    /// Create a user via the admin API (no Cognito email sending).
    /// Sets a permanent password immediately so the user is CONFIRMED.
    /// Returns the Cognito user `sub`.
    pub async fn admin_create_user(
        &self,
        username: &str,
        password: &str,
        email: &str,
    ) -> Result<String, DalError> {
        use aws_sdk_cognitoidentityprovider::types::MessageActionType;

        let resp = self
            .inner
            .admin_create_user()
            .user_pool_id(&self.pool_id)
            .username(username)
            .message_action(MessageActionType::Suppress)
            .user_attributes(
                aws_sdk_cognitoidentityprovider::types::AttributeType::builder()
                    .name("email")
                    .value(email)
                    .build()
                    .context("build email attr")
                    .map_err(|e| {
                        self.cognito_context_error(
                            "admin_create_user:build_email_attr",
                            Some(username),
                            e,
                        )
                    })?,
            )
            .send()
            .await
            .map_err(|e| self.cognito_request_error("admin_create_user", Some(username), e))?;

        let prepared = async {
            let sub = resp
                .user()
                .and_then(|user| {
                    user.attributes()
                        .iter()
                        .find(|attribute| attribute.name() == "sub")
                        .and_then(|attribute| attribute.value())
                })
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    DalError::Cognito("admin_create_user returned no subject identifier".into())
                })?
                .to_owned();
            // Reuse the password contract used by reset; this confirms the new identity.
            self.admin_set_password(username, password).await?;
            Ok(sub)
        }
        .await;
        if prepared.is_err() && self.admin_delete_user_if_exists(username).await.is_err() {
            // AdminCreate succeeded, but no DB identity can be created after this error.
            tracing::error!(
                username,
                operation = "admin_create_user:rollback",
                "new Cognito identity requires reconciliation"
            );
        }
        prepared
    }

    /// Sign up a new user. Does NOT send a Cognito verification email
    /// (we handle that via Mailjet). Admin confirms the user separately
    /// after our own email verification.
    pub async fn sign_up(
        &self,
        username: &str,
        password: &str,
        email: &str,
    ) -> Result<String, DalError> {
        let resp = self
            .inner
            .sign_up()
            .client_id(&self.client_id)
            .username(username)
            .password(password)
            .user_attributes(
                aws_sdk_cognitoidentityprovider::types::AttributeType::builder()
                    .name("email")
                    .value(email)
                    .build()
                    .context("build email attr")
                    .map_err(|e| {
                        self.cognito_context_error("sign_up:build_email_attr", Some(username), e)
                    })?,
            )
            .send()
            .await
            .map_err(|e| self.cognito_request_error("sign_up", Some(username), e))?;

        let sub = resp.user_sub().to_string();
        Ok(sub)
    }

    /// Manually confirm a user in Cognito (called after our email verification flow).
    pub async fn admin_confirm_user(&self, username: &str) -> Result<(), DalError> {
        // Permanent-password registration already confirms Cognito. Legacy SignUp
        // identities still need confirmation; don't swallow unrelated provider errors.
        let user = self
            .inner
            .admin_get_user()
            .user_pool_id(&self.pool_id)
            .username(username)
            .send()
            .await
            .map_err(|error| {
                self.cognito_request_error("admin_get_user:confirmation", Some(username), error)
            })?;
        if user.user_status()
            == Some(&aws_sdk_cognitoidentityprovider::types::UserStatusType::Confirmed)
        {
            return Ok(());
        }
        self.inner
            .admin_confirm_sign_up()
            .user_pool_id(&self.pool_id)
            .username(username)
            .send()
            .await
            .map_err(|e| self.cognito_request_error("admin_confirm_sign_up", Some(username), e))?;
        Ok(())
    }

    /// Set a user's password (used for our password-reset flow).
    pub async fn admin_set_password(
        &self,
        username: &str,
        new_password: &str,
    ) -> Result<(), DalError> {
        self.inner
            .admin_set_user_password()
            .user_pool_id(&self.pool_id)
            .username(username)
            .password(new_password)
            .permanent(true)
            .send()
            .await
            .map_err(|e| {
                self.cognito_request_error("admin_set_user_password", Some(username), e)
            })?;
        Ok(())
    }

    /// Delete a user from Cognito (account deletion).
    pub async fn admin_delete_user(&self, username: &str) -> Result<(), DalError> {
        self.inner
            .admin_delete_user()
            .user_pool_id(&self.pool_id)
            .username(username)
            .send()
            .await
            .map_err(|e| self.cognito_request_error("admin_delete_user", Some(username), e))?;
        Ok(())
    }

    /// Delete a user from Cognito, treating "already gone" as a successful no-op.
    /// Returns `true` if Cognito deleted a user, `false` if the user was already absent.
    pub async fn admin_delete_user_if_exists(&self, username: &str) -> Result<bool, DalError> {
        match self
            .inner
            .admin_delete_user()
            .user_pool_id(&self.pool_id)
            .username(username)
            .send()
            .await
        {
            Ok(_) => Ok(true),
            Err(err) => {
                if err
                    .as_service_error()
                    .is_some_and(|e| e.is_user_not_found_exception())
                {
                    warn!(
                        operation = "admin_delete_user",
                        username,
                        pool_id = %self.pool_id,
                        client_id = %self.client_id,
                        "cognito user already absent during cleanup"
                    );
                    Ok(false)
                } else {
                    Err(self.cognito_request_error("admin_delete_user", Some(username), err))
                }
            }
        }
    }

    /// Sign out (globally invalidate all tokens) for a user.
    pub async fn admin_sign_out(&self, username: &str) -> Result<(), DalError> {
        self.inner
            .admin_user_global_sign_out()
            .user_pool_id(&self.pool_id)
            .username(username)
            .send()
            .await
            .map_err(|e| {
                self.cognito_request_error("admin_user_global_sign_out", Some(username), e)
            })?;
        Ok(())
    }

    fn cognito_request_error<E>(
        &self,
        operation: &'static str,
        _username: Option<&str>,
        _err: E,
    ) -> DalError
    where
        E: Debug + Display,
    {
        error!(operation, "cognito sdk request failed");
        DalError::Cognito(format!("cognito {operation} failed"))
    }

    fn cognito_context_error<E>(
        &self,
        operation: &'static str,
        _username: Option<&str>,
        _err: E,
    ) -> DalError
    where
        E: Debug + Display,
    {
        error!(operation, "cognito request setup failed");
        DalError::Cognito(format!("cognito {operation} failed"))
    }
}
#[cfg(test)]
mod mfa_tests {
    use super::*;
    #[test]
    fn authentication_never_confuses_challenges_with_completed_login() {
        let challenge = ChallengeNameType::SoftwareTokenMfa;
        assert!(matches!(
            authentication(
                None,
                Some(&challenge),
                Some("fixture-session"),
                Some("fixture")
            ),
            Ok(Authentication::Totp { .. })
        ));
        assert!(authentication(None, Some(&challenge), None, Some("fixture")).is_err());
        assert!(
            authentication(
                None,
                Some(&ChallengeNameType::MfaSetup),
                Some("fixture-session"),
                Some("fixture")
            )
            .is_err()
        );
        assert!(
            authentication(
                None,
                Some(&ChallengeNameType::SmsMfa),
                Some("fixture-session"),
                Some("fixture")
            )
            .is_err()
        );
        let tokens = aws_sdk_cognitoidentityprovider::types::AuthenticationResultType::builder()
            .access_token("access")
            .id_token("id")
            .refresh_token("refresh")
            .build();
        assert!(matches!(
            authentication(Some(&tokens), None, None, None),
            Ok(Authentication::Complete(_))
        ));
        assert!(
            authentication(
                Some(&tokens),
                Some(&challenge),
                Some("fixture-session"),
                Some("fixture")
            )
            .is_err()
        );
        let missing = aws_sdk_cognitoidentityprovider::types::AuthenticationResultType::builder()
            .id_token("id")
            .build();
        assert!(authentication(Some(&missing), None, None, None).is_err());
        let injected = aws_sdk_cognitoidentityprovider::types::AuthenticationResultType::builder()
            .access_token("access")
            .id_token("id")
            .refresh_token("value;cookie=injection")
            .build();
        assert!(authentication(Some(&injected), None, None, None).is_err());
    }
}
