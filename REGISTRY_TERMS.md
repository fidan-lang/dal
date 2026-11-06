# Official DAL registry service terms

Last updated: 6 November 2026

These terms describe the use of the official hosted DAL package registry at
`dal.fidan.dev` and `api.dal.fidan.dev`. They concern the hosted service, not
independent installations or the rights granted by open-source licenses.

## 1. Software, packages, and hosted service

- **DAL software:** DAL is licensed under [Apache License 2.0](LICENSE). You may
  use, modify, redistribute, sell, and independently host the licensed software,
  including for competing registries, subject to that license.
- **Published packages:** Package authors and other rights holders retain their
  rights. Each package's own license governs its use and redistribution.
  Publishing a package does not transfer its ownership to the DAL operator.
  Publishers must have permission to distribute their content.
- **Hosted registry:** These service terms address access to infrastructure
  operated for the official DAL registry. They do not override software or
  package licenses, mandatory law, or lawful independent distribution.

## 2. Free and legitimate use

Public package downloads, metadata queries, dependency resolution, builds,
CI/CD, local caches, and legitimate development tooling are welcome. Public
package downloads do not require an account. Publishing and account management
require the authentication and authorization specified by the registry.

You may mirror or redistribute packages to the extent their licenses and
applicable law permit. Running an independent registry, commercial use, or
ordinary automated access is not inherently abuse. For sustained high-volume
synchronization, coordinate with the maintainers so shared infrastructure can
remain reliable.

## 3. Acceptable use and security

Do not deliberately overload or disrupt the hosted service. Avoid abusive
requests, scraping, or bulk downloads that degrade availability or deliberately
circumvent proportionate technical rate limits. Respect throttling and retry
signals; use caches, bounded concurrency, and backoff where appropriate.

Do not attempt unauthorized access to accounts, private data, storage
credentials, administration tools, or underlying infrastructure. Do not bypass
access controls, falsify authorization, or use download links to access storage
outside their intended scope. Do not publish packages you are not authorized to
redistribute or material that violates applicable law.

This policy does not claim exclusive ownership of public package metadata,
prohibit lawful mirrors, or limit rights independently granted by package
licenses. Database-related rights and statutory exceptions remain governed by
applicable law, including the relevant German and EU provisions.

## 4. Service operation and enforcement

The operator may use proportionate rate limits, temporary restrictions,
security controls, package moderation, and other lawful measures to protect
service availability and users. Legitimate shared-IP, CI, proxy, and mirror
traffic should be considered when investigating suspected abuse.

An infrastructure restriction does not terminate an Apache 2.0 license or
revoke rights under an independently licensed package. The hosted service is
provided without a guaranteed service level or perpetual hosting commitment.

## 5. Updates and contact

The current service policy is published in this repository. Material changes
will be published with an updated date, and any additional notice or consent
required by applicable law will be handled separately. Anonymous downloads do
not by themselves constitute an agreement to unrelated contractual terms.

For questions, operational coordination, or reports of misuse, use the
[DAL repository](https://github.com/fidan-lang/dal). Report vulnerabilities
privately using the process in [SECURITY.md](SECURITY.md). Never publish
passwords, recovery codes, private user data, or security credentials in issues.
