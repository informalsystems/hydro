## Cutting a new release

### Creating the changelog

To create a new release, you will need to have [unclog](https://github.com/informalsystems/unclog) installed.

First, run:
```
unclog release vX.Y.Z --editor nano
```
This will move all the changelog entries from the `unreleased` folder into a new folder named after the release tag. It will also open an editor for you to write the release notes.
For the release notes, include the date (as in `Date: October 15th, 2024`). You can
optionally add a short summary of the release, but do not duplicate the changelog entries.

Then, regenerate the `CHANGELOG` by running
```
unclog build > CHANGELOG
```
Finally, commit and push the changes to the repo.
The up-to-date changelog should be present on the release branch for the release you have just cut,
and also on main. 

### Updating versions

Modify the versions in the `Cargo.toml` files for all contracts that the release is for.

### Migration

Make sure that the migration entrypoint in `contract/src/migration/migrate.rs` has the correct behaviour for the upgrade.
That means it uses the right Migration message, and calls the right migration function.
If the contract has to be migrated due to the API or other code changes, but it doesn't require any state migrations, make sure that the given contract has `migrate()` entry point defined. Otherwise, migration will not be possible.

### Building/Releasing

Run `make compile` to regenerate the contracts with the new version number.

### Cutting the release

Create a PR with these changes, in the usual cases targetting main.
After the changes are merged into main, push the changes from main to the release branch, e.g. `release/v3.x` (Note: when merging a PR from main into release branch, make sure to use "merge" option instead of "squash and merge". This allows us to keep the history clean and reuse the same release branch for future releases).
Then, on Github, create a new tag from the branch, and release that tag.
As summary, use the CHANGELOG entry for that release, e.g.
```
## v3.1.1
Date: Feburary 25th, 2025

### FEATURE

- Allow voting with locks that voted for a proposal which did not receive any funds in its deployment
  ([\#231](https://github.com/informalsystems/hydro/pull/231))
```