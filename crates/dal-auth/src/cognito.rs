use anyhow::Context;
use aws_sdk_cognitoidentityprovider::{Client, types::AuthFlowType};
use dal_common::error::DalError;
use std::fmt::{Debug, Display};
use tracing::{error, warn};

/// Thin wrapper around the AWS Cognito Identity Provider SDK client.
pub struct CognitoClient {
    inner: Client,
    pool_id: String,
    client_id: String,
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
    /// Returns (access_token, id_token, refresh_token).
    pub async fn sign_in(
        &self,
        username: &str,
        password: &str,
    ) -> Result<(String, String, String), DalError> {
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
                self.cognito_request_error("initiate_auth:user_password", Some(username), e)
            })?;

        let result = resp.authentication_result().ok_or_else(|| {
            warn!(
                operation = "initiate_auth:user_password",
                username, "cognito returned no authentication result"
            );
            DalError::Cognito(
                "cognito initiate_auth:user_password returned no authentication result".into(),
            )
        })?;

        let access = result.access_token().unwrap_or_default().to_string();
        let id = result.id_token().unwrap_or_default().to_string();
        let refresh = result.refresh_token().unwrap_or_default().to_string();

        Ok((access, id, refresh))
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
