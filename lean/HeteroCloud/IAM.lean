namespace HeteroCloud.IAM

/-!
The authorization kernel receives facts produced by the policy matcher.
Keeping this final decision small makes the security invariants explicit and
allows the Rust evaluator to share a stable truth table with Lean.
-/

inductive Decision where
  | allow
  | deny
  deriving BEq, Repr

def authorize
    (sameOrganization applicableAllow applicableDeny : Bool) : Decision :=
  if !sameOrganization then
    .deny
  else if applicableDeny then
    .deny
  else if applicableAllow then
    .allow
  else
    .deny

theorem crossOrganizationDenied (allow deny : Bool) :
    authorize false allow deny = .deny := by
  simp [authorize]

theorem explicitDenyWins (sameOrganization allow : Bool) :
    authorize sameOrganization allow true = .deny := by
  cases sameOrganization <;> simp [authorize]

theorem defaultDeny :
    authorize true false false = .deny := by
  simp [authorize]

theorem allowRequiresAllGuards
    (sameOrganization applicableAllow applicableDeny : Bool)
    (permitted :
      authorize sameOrganization applicableAllow applicableDeny = .allow) :
    sameOrganization = true ∧
      applicableAllow = true ∧
      applicableDeny = false := by
  cases sameOrganization <;>
    cases applicableAllow <;>
      cases applicableDeny <;>
        simp [authorize] at permitted ⊢

end HeteroCloud.IAM

