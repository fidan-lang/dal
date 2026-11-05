/** Visible validation is advisory; the API independently validates before touching Cognito. */
export const PASSWORD_POLICY_HINT =
  "Use at least 8 characters with uppercase and lowercase letters, a number, and a special character.";

function hasAsciiPunctuation(value: string): boolean {
  for (const ch of value) {
    const code = ch.codePointAt(0)!;
    if (
      (code >= 33 && code <= 47) ||
      (code >= 58 && code <= 64) ||
      (code >= 91 && code <= 96) ||
      (code >= 123 && code <= 126)
    ) {
      return true;
    }
  }
  return false;
}

export function passwordPolicyError(password: string): string | null {
  const missing: string[] = [];
  if ([...password].length < 8) missing.push("at least 8 characters");
  if (!/[A-Z]/.test(password)) missing.push("an uppercase letter");
  if (!/[a-z]/.test(password)) missing.push("a lowercase letter");
  if (!/[0-9]/.test(password)) missing.push("a number");
  if (!hasAsciiPunctuation(password)) missing.push("a special character");
  return missing.length ? `Password must contain ${missing.join(", ")}.` : null;
}
