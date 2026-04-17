# 2026-04-17 Plan Document Audit Report

**Date:** 2026-04-17  
**Scope:** All 2026-04-17 plan documents (task1..task5) + outline  
**Methodology:** Overlap analysis, gap detection, cross-link verification, acceptance criteria review  
**Status:** Document audit only (no implementation state reviewed)

---

## Executive Summary

| Category | Status | Severity |
|----------|--------|----------|
| **Overlap Detection** | 3 duplicated work areas identified | MEDIUM |
| **Missing Cross-Links** | 4 critical doc dependencies unlinked | HIGH |
| **Acceptance Criteria** | 8 tasks lack explicit success metrics | MEDIUM |
| **Roadmap Gaps** | 2 areas absent from task coverage | MEDIUM |
| **Script Ownership** | 3 conflicting script definitions | HIGH |

---

## 1. OVERLAP & DUPLICATION ANALYSIS

### 1.1 Duplicated Work: `docs/SECURITY.md` Creation

**Affected Tasks:**
- **Task 1 (Release Hardening):** Task 8 - "编写 RELEASE.md 第一版" (creates docs/RELEASE.md)
- **Task 3 (Security Hardening):** Task 2 - "编写 SECURITY.md 第一版" (creates docs/SECURITY.md)
- **Task 4 (Docs Onboarding):** Task 10 - "统一文档互相引用关系" (modifies docs/SECURITY.md)

**Issue:** Task 3 creates `docs/SECURITY.md` from scratch, but Task 4 assumes it exists and modifies it. Task 4 should explicitly depend on Task 3 completion.

**Recommendation:** 
- Task 4 must execute AFTER Task 3
- Task 4 Task 10 should reference Task 3 output, not assume pre-existence

---

### 1.2 Duplicated Work: `docs/TROUBLESHOOTING.md` Creation

**Affected Tasks:**
- **Task 3 (Security Hardening):** Task 9 - "把支持流程写进文档" (modifies docs/TROUBLESHOOTING.md)
- **Task 4 (Docs Onboarding):** Task 7-9 - Creates and modifies docs/TROUBLESHOOTING.md

**Issue:** Task 3 assumes TROUBLESHOOTING.md exists and modifies it (Task 9), but Task 4 creates it from scratch (Task 7). Execution order conflict.

**Recommendation:**
- Task 4 must create TROUBLESHOOTING.md BEFORE Task 3 can modify it
- Task 3 should be reordered to execute after Task 4, OR
- Task 3 Task 9 should be moved to Task 4 as part of the same document creation flow

---

### 1.3 Duplicated Work: `package.json` Script Unification

**Affected Tasks:**
- **Task 1 (Release Hardening):** Task 1-3 - Audits and unifies scripts (`build`, `test`, `test:unit`, `check`)
- **Task 2 (Testing Hardening):** Task 1-2 - Audits frontend testing and adds `test:unit`, `test:e2e` scripts
- **Task 4 (Docs Onboarding):** Implicitly assumes scripts exist

**Issue:** Both Task 1 and Task 2 modify `package.json` scripts independently. Task 1 defines `test` and `test:unit`, Task 2 adds `test:unit` and `test:e2e`. Potential merge conflicts and unclear ownership.

**Recommendation:**
- Task 1 should own ALL script definitions (build, test, test:unit, test:e2e, check)
- Task 2 should only ADD test implementations, not redefine scripts
- Create explicit script ownership matrix (see Section 3.1)

---

## 2. MISSING CROSS-LINKS & DEPENDENCIES

### 2.1 Task Execution Order Not Documented

**Critical Dependency Chain:**
```
Task 1 (Release Hardening)
  ↓ (must complete before)
Task 2 (Testing Hardening) 
  ↓ (must complete before)
Task 3 (Security Hardening)
  ↓ (must complete before)
Task 4 (Docs Onboarding)
  ↓ (must complete before)
Task 5 (UX Polish)
```

**Issue:** No plan document explicitly states this dependency chain. Each task assumes prior tasks are complete but doesn't reference them.

**Missing Links:**
- Task 1 doesn't reference Task 2's test script needs
- Task 2 doesn't reference Task 1's CI workflow
- Task 3 doesn't reference Task 4's doc structure
- Task 4 doesn't reference Task 3's SECURITY.md creation
- Task 5 doesn't reference any prior tasks

**Recommendation:** Add explicit "Prerequisites" section to each task plan:
```markdown
## Prerequisites
- Task 1 must be complete (CI/Release pipeline established)
- package.json scripts must be unified
- docs/ directory structure must exist
```

---

### 2.2 Missing Cross-References in File Modifications

**Task 1 (Release Hardening) - Task 8:**
- Creates `docs/RELEASE.md`
- Should reference `docs/SECURITY.md` (created by Task 3)
- Should reference `docs/INSTALL.md` (created by Task 4)
- **Currently:** No cross-link strategy defined

**Task 3 (Security Hardening) - Task 9:**
- Modifies `docs/TROUBLESHOOTING.md`
- Should reference `docs/SECURITY.md` (created in same task)
- Should reference `docs/RELEASE.md` (created by Task 1)
- **Currently:** No cross-link strategy defined

**Task 4 (Docs Onboarding) - Task 10:**
- Explicitly handles cross-linking (Task 10: "统一文档互相引用关系")
- But assumes all docs exist (SECURITY.md, RELEASE.md, TROUBLESHOOTING.md)
- **Currently:** Depends on Task 1 and Task 3 completion

**Recommendation:** Create a "Documentation Cross-Link Matrix" in the outline:
```
README.md → INSTALL.md, TROUBLESHOOTING.md
INSTALL.md → README.md, TROUBLESHOOTING.md, SECURITY.md
TROUBLESHOOTING.md → SECURITY.md, RELEASE.md
SECURITY.md → THREAT_MODEL.md, TROUBLESHOOTING.md
RELEASE.md → SECURITY.md, INSTALL.md
```

---

### 2.3 Missing Reference to Outline Document

**Issue:** The outline document (2026-04-17-project-maturity-outline.md) is NOT referenced by any task plan.

- Outline defines P0/P1/P2 priorities
- Outline explains WHY these tasks exist
- Task plans don't reference the outline's strategic context

**Recommendation:** Each task should include:
```markdown
## Strategic Context
See: `.sisyphus/plans/2026-04-17-project-maturity-outline.md`
- This task addresses [P0/P1/P2] priority
- Contributes to: [specific maturity goal]
```

---

## 3. ACCEPTANCE CRITERIA & SUCCESS METRICS

### 3.1 Missing Explicit Success Criteria

**Task 1 (Release Hardening):**
- ✅ Task 13 has verification steps (pnpm run build, test, check, cargo test)
- ❌ No explicit "success = all CI workflows pass" metric
- ❌ No "success = version numbers synchronized" verification
- ❌ No "success = release.yml produces artifacts" verification

**Task 2 (Testing Hardening):**
- ✅ Task 14 has verification steps (pnpm test:unit, test:e2e, cargo test)
- ❌ No explicit "success = X% code coverage" metric
- ❌ No "success = all critical paths covered" definition
- ❌ No "success = E2E tests pass on CI" verification

**Task 3 (Security Hardening):**
- ✅ Task 12 has verification steps (run tests, verify non-sensitive exports)
- ❌ No explicit "success = threat model complete" metric
- ❌ No "success = all cleanup paths verified" definition
- ❌ No "success = Unix permissions hardened" verification

**Task 4 (Docs Onboarding):**
- ✅ Task 11 has walkthrough verification
- ❌ No explicit "success = new user can install from README" metric
- ❌ No "success = all docs cross-linked" verification
- ❌ No "success = no broken links" check

**Task 5 (UX Polish):**
- ✅ Task 10 has UI walkthrough
- ❌ No explicit "success = keyboard-only navigation works" metric
- ❌ No "success = WCAG 2.1 AA compliance" target
- ❌ No "success = help system functional" verification

**Recommendation:** Add explicit "Definition of Done" to each task:
```markdown
## Definition of Done
- [ ] All steps completed and committed
- [ ] Local verification passed: [specific commands]
- [ ] CI workflow passes (if applicable)
- [ ] Success metric: [measurable outcome]
- [ ] Handoff documentation updated
```

---

### 3.2 Script Ownership Matrix (CRITICAL)

**Current State - CONFLICTING:**

| Script | Task 1 | Task 2 | Current Status |
|--------|--------|--------|-----------------|
| `build` | Defines | - | ✅ Unified |
| `test` | Defines | Assumes exists | ⚠️ CONFLICT |
| `test:unit` | Defines | Adds implementation | ⚠️ CONFLICT |
| `test:e2e` | - | Adds | ❌ MISSING |
| `check` | Defines | - | ✅ Unified |

**Issue:** 
- Task 1 Task 2 says: "添加缺失的 `test:unit` 脚本" (add missing test:unit)
- Task 2 Task 2 says: "在 package.json 中增加 `test:unit` 脚本" (add test:unit)
- Both tasks claim ownership of the same script

**Recommendation:** Clarify ownership:
```markdown
## Script Ownership

### Task 1 Responsibility (Release Hardening)
- Define: build, test, test:unit, check
- These are ENTRY POINTS only, may be empty/placeholder

### Task 2 Responsibility (Testing Hardening)
- Implement: test:unit (Vitest), test:e2e (Playwright)
- Modify: test script to aggregate all test types
- DO NOT redefine script names, only add implementations
```

---

## 4. ROADMAP GAPS & MISSING AREAS

### 4.1 Missing: Updater & Auto-Update Infrastructure

**Mentioned in Outline:**
- Section 5 (P2 priorities): "更强的平台覆盖 / updater / 后续扩展能力"

**Coverage in Tasks:**
- ❌ Task 1: No updater setup
- ❌ Task 2: No updater testing
- ❌ Task 3: No updater security considerations
- ❌ Task 4: No updater documentation
- ❌ Task 5: No updater UX

**Issue:** Updater is mentioned as P2 but has NO task coverage. If it's truly P2, it should be deferred explicitly. If it's needed sooner, it needs a task.

**Recommendation:** Add to outline or create Task 6:
```markdown
## Deferred to Future Phase
- Updater infrastructure (P2)
- i18n full implementation (P2)
- Dark mode full implementation (P2)
- Platform-specific installers (P2)
```

---

### 4.2 Missing: Cargo Workspace Version Synchronization

**Mentioned in Task 1:**
- Task 7: "审计并记录所有版本字段"
- Task 9: "实际对齐版本号"

**Coverage:**
- ✅ Task 1 handles version alignment
- ❌ No task handles Cargo workspace version sync strategy
- ❌ No task defines "single source of truth" for versions

**Issue:** Task 1 lists files to modify but doesn't define HOW to keep them in sync going forward.

**Recommendation:** Task 1 should include:
```markdown
## Version Sync Strategy
- Single source of truth: [file]
- Sync mechanism: [manual/script/CI]
- Verification: [how to check alignment]
```

---

### 4.3 Missing: Platform-Specific Build & Test Matrix

**Mentioned in Task 1:**
- Task 5: "为 CI 增加平台矩阵与分层 job"

**Coverage:**
- ✅ Task 1 adds Windows/macOS matrix to CI
- ❌ No task defines platform-specific test requirements
- ❌ No task covers platform-specific signing/notarization
- ❌ No task covers platform-specific troubleshooting

**Issue:** Task 1 builds the matrix but doesn't define what each platform should test.

**Recommendation:** Task 1 should include:
```markdown
## Platform Matrix Definition
- Windows: [specific test requirements]
- macOS: [specific test requirements]
- Linux: [optional/future]
```

---

## 5. DOCUMENT STRUCTURE & CLARITY ISSUES

### 5.1 Inconsistent File Path References

**Task 1:**
- Uses: `docs/RELEASE.md`
- Uses: `.github/workflows/ci.yml`

**Task 2:**
- Uses: `vitest.config.ts`
- Uses: `tests/e2e/critical-flows.spec.ts`

**Task 3:**
- Uses: `docs/SECURITY.md`
- Uses: `crates/bw-agent/src/access_log.rs`

**Task 4:**
- Uses: `README.md` (root)
- Uses: `docs/INSTALL.md`

**Issue:** Inconsistent path depth and directory structure. No unified file structure diagram.

**Recommendation:** Add to outline:
```markdown
## Project File Structure (Post-Implementation)
```
.
├── README.md
├── docs/
│   ├── INSTALL.md
│   ├── SECURITY.md
│   ├── THREAT_MODEL.md
│   ├── TROUBLESHOOTING.md
│   ├── RELEASE.md
│   └── superpowers/
├── .github/workflows/
│   ├── ci.yml
│   └── release.yml
├── vitest.config.ts
├── tests/
│   └── e2e/
└── src-tauri/
    └── tests/
```
```

---

### 5.2 Inconsistent Terminology

**Task 1:** "发布工程与 CI 细化实施计划"  
**Task 2:** "测试体系补强细化实施计划"  
**Task 3:** "安全与运维硬化细化实施计划"  
**Task 4:** "用户文档与 Onboarding 细化实施计划"  
**Task 5:** "UX 打磨与可访问性细化实施计划"

**Issue:** Inconsistent naming pattern. Task 1 uses "发布工程", Task 3 uses "运维硬化", Task 4 uses "Onboarding" (English).

**Recommendation:** Standardize terminology:
- Task 1: Release Engineering & CI Hardening
- Task 2: Testing Hardening
- Task 3: Security & Operability Hardening
- Task 4: User Documentation & Onboarding
- Task 5: UX Polish & Accessibility

---

## 6. CRITICAL ISSUES REQUIRING RESOLUTION

### 6.1 🔴 HIGH: Task Execution Order Undefined

**Problem:** No explicit execution order defined. Plans assume sequential execution but don't state it.

**Impact:** 
- Parallel execution might cause conflicts (e.g., both Task 1 and Task 2 modifying package.json)
- Circular dependencies possible (Task 3 needs Task 4's docs, Task 4 needs Task 3's docs)

**Resolution Required:**
```markdown
## Execution Order (MANDATORY)
1. Task 1 (Release Hardening) - Establishes CI/build foundation
2. Task 2 (Testing Hardening) - Adds test implementations
3. Task 3 (Security Hardening) - Adds security features & docs
4. Task 4 (Docs Onboarding) - Creates user-facing docs
5. Task 5 (UX Polish) - Final UX enhancements
```

---

### 6.2 🔴 HIGH: Script Ownership Conflict

**Problem:** Task 1 and Task 2 both claim to define `test` and `test:unit` scripts.

**Impact:**
- Merge conflicts if executed in parallel
- Unclear which task is responsible for script maintenance
- CI integration ambiguous

**Resolution Required:**
```markdown
## Script Definition Ownership
- Task 1: Define all script NAMES and ENTRY POINTS
- Task 2: Implement test IMPLEMENTATIONS only
- Task 1 must complete BEFORE Task 2 modifies package.json
```

---

### 6.3 🔴 HIGH: Documentation Circular Dependency

**Problem:** 
- Task 3 creates SECURITY.md and modifies TROUBLESHOOTING.md
- Task 4 creates TROUBLESHOOTING.md and modifies SECURITY.md
- Circular dependency: Task 3 → Task 4 → Task 3

**Impact:**
- Execution order ambiguous
- Risk of incomplete documentation
- Cross-links may be broken

**Resolution Required:**
```markdown
## Documentation Creation Order
1. Task 4 creates: README.md, INSTALL.md, TROUBLESHOOTING.md
2. Task 3 creates: SECURITY.md, THREAT_MODEL.md
3. Task 4 Task 10 links all documents together
```

---

## 7. MISSING ACCEPTANCE CRITERIA BY TASK

### Task 1 (Release Hardening)
**Missing:**
- [ ] "Success = CI workflow passes on all commits"
- [ ] "Success = version numbers synchronized across all files"
- [ ] "Success = release.yml produces signed artifacts"
- [ ] "Success = docs/RELEASE.md is complete and accurate"

### Task 2 (Testing Hardening)
**Missing:**
- [ ] "Success = pnpm test:unit passes with >50% coverage"
- [ ] "Success = pnpm test:e2e passes on critical flows"
- [ ] "Success = cargo test passes with no regressions"
- [ ] "Success = all test scripts integrated into CI"

### Task 3 (Security Hardening)
**Missing:**
- [ ] "Success = SECURITY.md and THREAT_MODEL.md complete"
- [ ] "Success = access log cleanup verified"
- [ ] "Success = diagnostics export contains no sensitive data"
- [ ] "Success = Unix socket permissions hardened"

### Task 4 (Docs Onboarding)
**Missing:**
- [ ] "Success = new user can install from README alone"
- [ ] "Success = all docs cross-linked with no broken links"
- [ ] "Success = TROUBLESHOOTING covers all common issues"
- [ ] "Success = docs pass readability review"

### Task 5 (UX Polish)
**Missing:**
- [ ] "Success = keyboard-only navigation works end-to-end"
- [ ] "Success = all ARIA labels present on critical components"
- [ ] "Success = help system functional and discoverable"
- [ ] "Success = no accessibility violations detected"

---

## 8. RECOMMENDATIONS SUMMARY

| Priority | Category | Action | Owner |
|----------|----------|--------|-------|
| 🔴 HIGH | Execution Order | Define explicit sequential order in outline | Outline |
| 🔴 HIGH | Script Ownership | Clarify Task 1 vs Task 2 responsibility | Task 1 & 2 |
| 🔴 HIGH | Doc Dependencies | Resolve Task 3 ↔ Task 4 circular dependency | Task 3 & 4 |
| 🟡 MEDIUM | Cross-Links | Add "Prerequisites" section to each task | All tasks |
| 🟡 MEDIUM | Success Metrics | Add "Definition of Done" to each task | All tasks |
| 🟡 MEDIUM | File Structure | Create unified file structure diagram | Outline |
| 🟡 MEDIUM | Roadmap Gaps | Explicitly defer P2 items or create Task 6 | Outline |
| 🟢 LOW | Terminology | Standardize naming across tasks | All tasks |

---

## 9. DETAILED FINDINGS BY DOCUMENT

### 9.1 Outline (2026-04-17-project-maturity-outline.md)

**Strengths:**
- ✅ Clear strategic rationale for hardening focus
- ✅ Explicit P0/P1/P2 prioritization
- ✅ Acknowledges current state vs. desired state

**Gaps:**
- ❌ Doesn't reference the 5 task plans
- ❌ Doesn't define execution order
- ❌ Doesn't provide file structure diagram
- ❌ Doesn't define success metrics for each phase

**Recommendation:** Add section:
```markdown
## Implementation Plans
See `.sisyphus/plans/` for detailed task plans:
1. 2026-04-17-release-hardening-task1.md
2. 2026-04-17-testing-hardening-task2.md
3. 2026-04-17-security-hardening-task3.md
4. 2026-04-17-docs-onboarding-task4.md
5. 2026-04-17-ux-polish-task5.md

Execution order: Task 1 → Task 2 → Task 3 → Task 4 → Task 5
```

---

### 9.2 Task 1 (Release Hardening)

**Strengths:**
- ✅ Clear chunk structure (5 chunks)
- ✅ Detailed step-by-step tasks
- ✅ Explicit file modifications listed
- ✅ Verification steps included

**Gaps:**
- ❌ Doesn't reference Task 2's test script needs
- ❌ Doesn't define version sync strategy for future
- ❌ Platform matrix defined but not detailed
- ❌ No explicit success criteria

**Critical Issue:** Task 2 and Task 1 both modify package.json scripts

---

### 9.3 Task 2 (Testing Hardening)

**Strengths:**
- ✅ Clear progression from unit → component → E2E → integration
- ✅ Specific component targets identified
- ✅ Verification steps included

**Gaps:**
- ❌ Doesn't reference Task 1's script definitions
- ❌ Doesn't define coverage targets
- ❌ Doesn't reference CI integration from Task 1
- ❌ No explicit success criteria

**Critical Issue:** Assumes package.json scripts exist but Task 1 also defines them

---

### 9.4 Task 3 (Security Hardening)

**Strengths:**
- ✅ Clear security focus areas
- ✅ Threat model documentation included
- ✅ Verification steps included

**Gaps:**
- ❌ Assumes TROUBLESHOOTING.md exists (created by Task 4)
- ❌ Doesn't reference Task 4's doc structure
- ❌ No explicit success criteria
- ❌ Unix socket permissions strategy unclear

**Critical Issue:** Circular dependency with Task 4 on TROUBLESHOOTING.md

---

### 9.5 Task 4 (Docs Onboarding)

**Strengths:**
- ✅ Clear doc structure (README, INSTALL, TROUBLESHOOTING)
- ✅ Cross-linking explicitly handled (Task 10)
- ✅ New user walkthrough included

**Gaps:**
- ❌ Assumes SECURITY.md exists (created by Task 3)
- ❌ Assumes RELEASE.md exists (created by Task 1)
- ❌ Doesn't reference Task 3's doc creation
- ❌ No explicit success criteria

**Critical Issue:** Circular dependency with Task 3 on SECURITY.md

---

### 9.6 Task 5 (UX Polish)

**Strengths:**
- ✅ Clear progression from a11y → help → theme
- ✅ Specific component targets identified
- ✅ UI walkthrough included

**Gaps:**
- ❌ Doesn't reference prior tasks
- ❌ No explicit accessibility targets (WCAG level)
- ❌ No explicit success criteria
- ❌ i18n/theme decision deferred but not clearly

**Minor Issue:** Least integrated with other tasks

---

## 10. CROSS-REFERENCE MATRIX

### Files Modified by Multiple Tasks

| File | Task 1 | Task 2 | Task 3 | Task 4 | Task 5 | Conflict? |
|------|--------|--------|--------|--------|--------|-----------|
| package.json | ✅ Modify | ✅ Modify | - | - | - | 🔴 YES |
| pnpm-lock.yaml | ✅ Modify | ✅ Modify | - | - | - | 🔴 YES |
| docs/SECURITY.md | - | - | ✅ Create | ✅ Modify | - | 🟡 MAYBE |
| docs/TROUBLESHOOTING.md | - | - | ✅ Modify | ✅ Create | - | 🔴 YES |
| docs/RELEASE.md | ✅ Create | - | - | ✅ Modify | - | 🟡 MAYBE |
| src/pages/SettingsPage.tsx | - | ✅ Modify | ✅ Modify | - | ✅ Modify | 🟡 MAYBE |
| src-tauri/Cargo.toml | ✅ Modify | ✅ Modify | - | - | - | 🟡 MAYBE |

---

## 11. FINAL VERDICT

### Overall Assessment: **REQUIRES REVISION BEFORE EXECUTION**

**Readiness Score: 6/10**

| Dimension | Score | Notes |
|-----------|-------|-------|
| Clarity | 7/10 | Clear individual tasks, unclear dependencies |
| Completeness | 6/10 | Missing success criteria, roadmap gaps |
| Consistency | 5/10 | Conflicting script ownership, circular deps |
| Testability | 7/10 | Good verification steps, missing metrics |
| Maintainability | 5/10 | No version sync strategy, unclear ownership |

---

### Critical Blockers (Must Fix Before Execution)

1. **🔴 Resolve Task 1 ↔ Task 2 script ownership conflict**
   - Define which task owns script definitions
   - Define which task owns implementations
   - Ensure no parallel modifications to package.json

2. **🔴 Resolve Task 3 ↔ Task 4 documentation circular dependency**
   - Clarify which task creates TROUBLESHOOTING.md
   - Clarify which task creates SECURITY.md
   - Define explicit execution order

3. **🔴 Define explicit execution order in outline**
   - State that tasks must execute sequentially
   - Document why parallel execution is not possible
   - Add prerequisites to each task

---

### Recommended Actions (Before Execution)

1. **Update Outline Document:**
   - Add execution order section
   - Add file structure diagram
   - Add success metrics for each phase
   - Reference all 5 task plans

2. **Update Each Task Plan:**
   - Add "Prerequisites" section
   - Add "Definition of Done" section
   - Add explicit success criteria
   - Add cross-references to other tasks

3. **Create Script Ownership Matrix:**
   - Task 1: Define all script names
   - Task 2: Implement test scripts only
   - Document in Task 1 plan

4. **Resolve Documentation Dependencies:**
   - Task 4 creates TROUBLESHOOTING.md (not Task 3)
   - Task 3 creates SECURITY.md (not Task 4)
   - Task 4 Task 10 links all documents
   - Document in both Task 3 and Task 4 plans

---

## 12. APPENDIX: DETAILED CROSS-LINK RECOMMENDATIONS

### Recommended Cross-Link Structure

```
README.md
├── → INSTALL.md (for installation)
├── → TROUBLESHOOTING.md (for help)
└── → SECURITY.md (for security info)

INSTALL.md
├── → README.md (for overview)
├── → TROUBLESHOOTING.md (for help)
└── → SECURITY.md (for security requirements)

TROUBLESHOOTING.md
├── → INSTALL.md (for reinstall)
├── → SECURITY.md (for security issues)
└── → RELEASE.md (for version info)

SECURITY.md
├── → THREAT_MODEL.md (for details)
├── → TROUBLESHOOTING.md (for diagnostics)
└── → RELEASE.md (for version security)

RELEASE.md
├── → SECURITY.md (for signing)
└── → INSTALL.md (for deployment)

THREAT_MODEL.md
└── → SECURITY.md (for implementation)
```

---

## 13. APPENDIX: MISSING ACCEPTANCE CRITERIA TEMPLATES

### Template for Task 1 (Release Hardening)

```markdown
## Acceptance Criteria

### Build & CI
- [ ] `pnpm run build` succeeds locally
- [ ] `pnpm run check` succeeds locally
- [ ] `.github/workflows/ci.yml` passes on all commits
- [ ] CI matrix includes Windows and macOS

### Version Alignment
- [ ] package.json version matches tauri.conf.json
- [ ] All Cargo.toml versions match
- [ ] Version sync strategy documented

### Release Documentation
- [ ] docs/RELEASE.md complete and accurate
- [ ] Signing/notarization prerequisites documented
- [ ] Platform differences documented

### Success Metric
- [ ] New contributor can run `pnpm run check` and see all tests pass
```

### Template for Task 2 (Testing Hardening)

```markdown
## Acceptance Criteria

### Test Infrastructure
- [ ] `pnpm test:unit` runs Vitest successfully
- [ ] `pnpm test:e2e` runs Playwright successfully
- [ ] `cargo test` passes with no regressions
- [ ] All test scripts integrated into CI

### Coverage
- [ ] Frontend unit tests cover >50% of src/lib
- [ ] Critical components have component tests
- [ ] Critical user flows have E2E tests

### Success Metric
- [ ] New contributor can run `pnpm test` and see all tests pass
```

---

**Report Generated:** 2026-04-17  
**Audit Scope:** Plans only (no implementation state)  
**Next Step:** Address critical blockers before execution

