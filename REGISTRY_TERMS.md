# Official DAL registry service terms

**Draft for legal review — not an automatically binding agreement.** These terms
must be reviewed and approved before publication as enforceable service terms.
Placing this file in a repository, or linking it from a footer, does not by itself
establish valid notice, consent, or contractual enforceability.

## 1. Scope: three separate sets of rights

1. **Server software.** The DAL implementation is open source under the standard
   [Apache License 2.0](LICENSE). You may use, modify, fork, redistribute, sell, and
   host it, including as an independent or competing commercial registry. These
   service terms do not amend that license or govern an independent operator's
   infrastructure. See [TRADEMARKS.md](TRADEMARKS.md) for branding guidance.
2. **Packages.** Authors and other rights holders retain their rights in uploaded
   packages. A package's own license governs its use and redistribution. Hosting
   does not transfer package ownership to DAL or give DAL exclusive rights.
   Publication should be performed only by someone entitled to publish the content.
3. **Official hosted service.** The acceptable-use policy below concerns access to
   infrastructure operated for the official public registry at `dal.fidan.dev`
   and `api.dal.fidan.dev`. It does not withdraw rights already granted in software
   or package licenses. Any enforceable access restriction remains subject to
   applicable law, including mandatory exceptions and user rights.

## 2. Normal use is welcome and intended to be free

The official public registry is intended to be free to use. Permitted activities
include installing and downloading public packages, publishing authorized packages,
normal dependency resolution and metadata requests, local/shared caches, builds,
CI/CD, reproducible installations, and legitimate automated development tooling.
Ordinary public downloads do not require a registry account or bearer token.

You may redistribute or mirror packages as their individual licenses permit.
Mirroring, automation, commercial activity, or operating another registry is not
by itself abuse, and not all mirroring or bulk access is unlawful. For sustained
high-volume synchronization, contact the maintainers to coordinate practical
budgets and caching; this request does not create a new permission requirement
for redistribution already authorized by a package license or applicable law.

## 3. Protect the shared hosted infrastructure

Do not deliberately disrupt or overwhelm the official service with excessive
requests or bulk downloads. Avoid abusive scraping that degrades availability,
creates disproportionate resource consumption, or continues in deliberate evasion
of proportionate technical limits. Use caches, bounded concurrency, and backoff
when the service signals temporary limits or failure.

Do not attempt unauthorized access to accounts, private data, storage credentials,
storage administration, or underlying infrastructure. Do not bypass technical
access controls, falsify authorization, or evade abuse mitigations to continue
harmful activity. Published download links authorize only their intended object
access; they are not permission to administer or enumerate private storage.

Systematic extraction of protected or nonpublic hosted database contents may be
restricted **only to the extent supported by applicable law or valid service
terms**. This is not a claim that all public registry metadata has an exclusive
owner, that all extraction is illegal, or that package licenses prohibit copying.
Statutory exceptions, independently obtained information, and authors' package
rights are unaffected.

### Bulk harvesting and possible database protection — counsel review required

Automated extraction or bulk mirroring of substantial portions of the official
registry through its hosted endpoints may be subject to reasonable technical
usage limits and restrictions in validly applicable terms, especially where it
burdens infrastructure or unlawfully circumvents access controls. That concerns
use of these servers, not lawful redistribution of packages obtained independently
or through authorized use. It does not categorically prohibit mirrors or competitors.

For Germany/EU, possible database-maker protection under Directive 96/9/EC Articles
7–8 and national law depends on proven facts, including a qualifying substantial
investment in obtaining, verifying or presenting the collection. Neither the
existence nor ownership of such rights is established here. Counsel must also
consider lawful users' rights, exceptions, Article 15, package licenses and
competition law before relying on any substantial or repeated-extraction claim.
See the [clause-by-clause review checklist](docs/LEGAL_REVIEW_REQUIRED.md).

## 4. Proportionate operational response

The operator may apply lawful, proportionate request limits, temporary throttling,
or restrictions on demonstrated abuse to keep the shared service available.
Normal shared-IP, proxy, mirror, and CI traffic should be considered before applying
limits; automation alone is not a reason to block access. Coordinate remediation
and review requests through the [project's contact channels](https://github.com/fidan-lang/dal).
Do not post credentials, private account data, or vulnerability details publicly;
use the reporting process in [SECURITY.md](SECURITY.md).

An infrastructure restriction does not revoke an Apache license or an existing
package license. These draft terms grant no new ownership or exclusive right in
third-party packages. No service-level guarantee or perpetual hosting commitment
is created here. Technical implementation and unapplied operator controls are
separated in [Registry security](docs/REGISTRY_SECURITY.md).

## 5. Required legal and publication review

Before treating any provision as enforceable, qualified legal review must:

- Confirm the actual operator/legal identity, contact details, jurisdictions,
  mandatory consumer/user rights, and any required privacy disclosures.
- Review notice, effective date, assent (where required), revisions, and the fact
  that unauthenticated downloads cannot be assumed to imply contractual consent.
- Review the scope and proportionality of anti-abuse/access restrictions and the
  protected-database extraction clause; do not assume database rights exist.
- Review restrictions, suspension/review procedures, availability statements, and
  any future dispute, liability, retention, or service-change provisions. No
  governing-law choice or broad waiver has been invented in this draft.
- Confirm consistency with Apache 2.0, package licenses, fair/nominative trademark
  use, applicable competition law, and independently operated registries.

Until that review and a valid publication process, use this document as a proposed
service policy, not as proof that a visitor accepted a contract.

## 6. Proposed notice process — not implemented or activated

After counsel and owner approval, registered publishers could receive the exact
approved service-policy version before accepting it, with a visible downloadable
copy and a minimal acceptance record. Changes would require a reviewed notice and
reacceptance policy. Such acceptance must not be described as an assignment of
package rights or a modification of Apache 2.0. No checkbox, consent record or
binding terms activation is introduced by this draft.

An anonymous downloader seeing a footer or using a public URL cannot automatically
be assumed to have agreed to a contract. Clear notices may explain proportionate
technical limits, but notice, assent and legal enforceability require separate
assessment; technical abuse protection must not depend on fictional consent.
