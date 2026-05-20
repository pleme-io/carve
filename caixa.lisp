;; carve — monolithic branch → ticket-aligned stacked-PR primitive.
;;
;; Substrate-promoted form of the carve pattern documented in
;; pleme-io/theory/CARVE.md.
;;
;; Provides a CLI `carve` that automates the operator side of the
;; monolithic-to-stack flow: JIRA-epic fan-out, commit-to-ticket
;; mapping, cross-cutting commit splits, BLAKE3-attested backup tags,
;; tree-hash equivalence gates, stack publication, JIRA sync.
;;
;; Build:
;;   nix build .#carve
;;   nix run  .#carve -- --help
;;   nix run  .#carve -- plan --epic ASM-18003
;;
;; Publish (caixa-publish via feira):
;;   feira publish               ; → tags vN.M.P, pushes to origin
;;
;; Consume via home-manager:
;;   imports = [ carve.homeManagerModules.default ];
;;   programs.carve.enable = true;

(defcaixa
  :nome        "carve"
  :versao      "0.1.0"
  :kind        Binario
  :edicao      "2026"
  :descricao   "Monolithic-branch → ticket-aligned stacked-PR primitive — JIRA epic fan-out, commit-to-ticket mapping, cross-cutting commit splits, BLAKE3-attested backups, tree-hash gates, stack publication."
  :repositorio "github:pleme-io/carve"
  :licenca     "MIT"
  :autores     ("pleme-io")
  :etiquetas   ("carve" "stacked-prs" "jira" "github" "git" "pre-pr" "caixa-binario")
  :deps        ()
  :deps-dev    ())
