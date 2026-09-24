export const meta = {
  name: 'review-candidate',
  description: 'Review the current change with four read-only lenses (correctness, design, performance, security), then adversarially verify each finding',
  whenToUse: 'Before handing a GitTurtle change to the verifier or opening a PR; run with args {base: "<git ref>"} to review the diff since that ref (default: merge-base with main)',
  phases: [
    { title: 'Review', detail: 'one read-only reviewer per lens' },
    { title: 'Verify', detail: 'one skeptic per finding tries to refute it' },
  ],
}

const base = (args && args.base) || 'main'
const MAX_VERIFIED = 8

const FINDINGS = {
  type: 'object',
  properties: {
    findings: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          file: { type: 'string' },
          line: { type: 'integer' },
          severity: { type: 'string', enum: ['blocker', 'major', 'minor'] },
          summary: { type: 'string' },
          failure_scenario: { type: 'string' },
          evidence: { type: 'string' },
        },
        required: ['file', 'line', 'severity', 'summary', 'failure_scenario'],
      },
    },
    clean: { type: 'boolean' },
  },
  required: ['findings', 'clean'],
}

const VERDICT = {
  type: 'object',
  properties: {
    refuted: { type: 'boolean' },
    reason: { type: 'string' },
  },
  required: ['refuted', 'reason'],
}

const LENSES = [
  { key: 'correctness', agentType: 'code-reviewer', focus: 'correctness bugs, regressions, weakened tests and broken repository-preservation, stale-target or generation guards' },
  { key: 'design', agentType: 'design-reviewer', focus: 'visible changes against DESIGN.md, resolved theme tokens, density, focus order, accessibility labels and whether native evidence exists for them' },
  { key: 'performance', agentType: 'performance-reviewer', focus: 'work moved onto the UI thread, lost virtualization, metadata no longer loading before content, unbounded reads or allocations, and missing release-mode measurements on hot paths' },
  { key: 'security', agentType: 'security-reviewer', focus: 'Git command construction, credentials and logs, decoder and resource bounds, filesystem and symlink handling, dependencies and CI trust' },
]

const scope = `Review the diff from \`git diff ${base}...HEAD\` plus the working tree (\`git diff\` and \`git status --porcelain\`). Read the crate guide for each touched crate before judging.`

const reviews = await pipeline(
  LENSES,
  lens => agent(
    `${scope}\n\nYour lens: ${lens.focus}. Report every finding you cannot rule out, with the file, the line, a concrete failure scenario and the evidence you saw; mark uncertain ones as minor. Set clean=true only if you found nothing that affects correctness, speed, appearance or safety. Do not edit anything.`,
    { label: `review:${lens.key}`, phase: 'Review', schema: FINDINGS, agentType: lens.agentType },
  ),
  (review, lens) => (review ? review.findings.map(f => ({ ...f, lens: lens.key })) : []),
)

const all = reviews.filter(Boolean).flat()
const order = { blocker: 0, major: 1, minor: 2 }
all.sort((a, b) => order[a.severity] - order[b.severity])
const toVerify = all.slice(0, MAX_VERIFIED)
if (all.length > MAX_VERIFIED) log(`${all.length - MAX_VERIFIED} lower-severity findings left unverified; they are returned as unverified`)
if (all.length === 0) log('all four lenses came back clean')

const verified = await pipeline(
  toVerify,
  f => agent(
    `${scope}\n\nA ${f.lens} reviewer claims: "${f.summary}" at ${f.file}:${f.line}. Failure scenario: ${f.failure_scenario}. Try to refute it by reading the actual code and its callers, or by running a narrow command such as a single test filter. Default to refuted=true if you cannot confirm the failure scenario from evidence. Do not edit anything.`,
    { label: `verify:${f.file}:${f.line}`, phase: 'Verify', schema: VERDICT },
  ),
  (verdict, f) => ({ ...f, refuted: verdict ? verdict.refuted : null, verdict_reason: verdict ? verdict.reason : 'verifier did not return' }),
)

return {
  base,
  confirmed: verified.filter(Boolean).filter(f => f.refuted === false),
  refuted: verified.filter(Boolean).filter(f => f.refuted === true),
  unverified: all.slice(MAX_VERIFIED),
}
