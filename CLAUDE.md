# Claude Instructions for Syncopate

## Project Philosophy

### Pre-1.0 Development

**Syncopate is in active pre-1.0 development.** This means:

- **Breaking changes are acceptable and encouraged** if they improve the architecture
- **API stability is NOT a priority** - evolve the design freely
- **Focus on finding the optimal architecture** rather than maintaining backward compatibility
- **Refactor aggressively** when a better approach is discovered
- **Don't be constrained by existing APIs** - if a better design emerges, implement it

### When Making Changes

- Prioritize correctness and elegant design over compatibility
- If you discover a better way to structure the code, propose or implement it
- Don't add complexity to maintain backward compatibility
- Feel free to rename, restructure, or completely redesign components
- Document breaking changes in commit messages, but don't avoid them

### Version 1.0 and Beyond

Once the project reaches 1.0:
- Semantic versioning will apply
- Breaking changes will require major version bumps
- Backward compatibility will become important
- Deprecation cycles will be used for API changes

**Until then: optimize the architecture without constraint.**

## Project Structure

- `crates/syncopate/` - Main scheduler library
  - `src/scheduler.rs` - Core scheduler implementation
  - `src/task.rs` - Task types and builders
  - `tests/` - Integration tests
  - `examples/` - Usage examples

## Current Focus

Building a high-performance task scheduler with:
- Virtual time simulation for testing
- Support for both relative (drift-based) and absolute (wall-clock aligned) timing
- Robust handling of time discontinuities (system sleep, clock adjustments)
- Clean, ergonomic API for task definition
