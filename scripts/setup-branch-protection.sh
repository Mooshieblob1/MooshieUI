#!/usr/bin/env bash
# Configures branch protection on Mooshieblob1/MooshieUI:main via the GitHub API.
#
# Prerequisites:
#   - gh CLI installed and authenticated: gh auth login
#   - Both the glassworm-scan AND pr-guardrails workflows must have each completed at
#     least one successful run so GitHub has registered both "GlassWorm Infection Audit"
#     and "Guardrails (build, i18n, types)" as known check names.
#     If branch protection is applied before the first run, all PRs will be permanently
#     blocked with "Required status check has never run."
#
# Run once from a developer workstation after cloning:
#   bash scripts/setup-branch-protection.sh

set -euo pipefail

REPO="Mooshieblob1/MooshieUI"
BRANCH="main"

# Verify gh is available and authenticated
if ! command -v gh &>/dev/null; then
    echo "ERROR: gh CLI not found. Install from https://cli.github.com/ and run 'gh auth login'."
    exit 1
fi

if ! gh auth status &>/dev/null; then
    echo "ERROR: gh CLI is not authenticated. Run 'gh auth login' first."
    exit 1
fi

echo "Applying branch protection to ${REPO}:${BRANCH}..."

gh api \
    --method PUT \
    "repos/${REPO}/branches/${BRANCH}/protection" \
    --input - <<'JSON'
{
  "required_status_checks": {
    "strict": true,
    "contexts": ["GlassWorm Infection Audit", "Guardrails (build, i18n, types)"]
  },
  "enforce_admins": true,
  "required_pull_request_reviews": {
    "dismiss_stale_reviews": true,
    "require_code_owner_reviews": true,
    "required_approving_review_count": 1
  },
  "restrictions": null,
  "allow_force_pushes": false,
  "allow_deletions": false,
  "block_creations": false
}
JSON

echo ""
echo "Branch protection applied. Verifying..."
echo ""

gh api "repos/${REPO}/branches/${BRANCH}/protection" --jq '{
  required_checks:       .required_status_checks.contexts,
  strict_up_to_date:     .required_status_checks.strict,
  required_reviews:      .required_pull_request_reviews.required_approving_review_count,
  dismiss_stale_reviews: .required_pull_request_reviews.dismiss_stale_reviews,
  codeowner_reviews:     .required_pull_request_reviews.require_code_owner_reviews,
  enforce_admins:        .enforce_admins.enabled,
  force_push_blocked:    (.allow_force_pushes.enabled | not),
  deletion_blocked:      (.allow_deletions.enabled | not)
}'

echo ""
echo "Done. All future PRs to main must:"
echo "  - Pass the 'GlassWorm Infection Audit' CI check"
echo "  - Pass the 'Guardrails (build, i18n, types)' CI check"
echo "  - Be up to date with main before merging"
echo "  - Have at least 1 approving review"
echo "  - Have CODEOWNERS approval for security-sensitive files"
echo "Force pushes and direct deletion of main are blocked (including for admins)."
