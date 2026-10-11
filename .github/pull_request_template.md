## Description
Briefly describe your changes or the new appliance recipe you are contributing.

## Type of Change
- [ ] 🎮 New Appliance Recipe (`recipes/<name>.yaml`)
- [ ] 🧪 Alpine Linux Musl Recipe
- [ ] ⚡ CLI / Build Tooling improvement
- [ ] 📖 Documentation / Asset improvement
- [ ] 🐛 Bug fix

## Verification Checklist
Before submitting, please ensure you have tested your change locally:
- [ ] Recipe passes schema validation: `./kurogane validate recipes/<name>.yaml`
- [ ] (If modifying code/QEMU) Appliance boots and runs: `./kurogane run recipes/<name>.yaml`
- [ ] (If modifying runtime) Regression suite verified: `./scripts/15_test_kurogane_cli.sh`
