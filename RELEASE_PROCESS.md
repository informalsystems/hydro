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