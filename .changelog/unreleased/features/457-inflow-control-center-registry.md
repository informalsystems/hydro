- Add an admin-controlled control center registry for the Inflow proxy, allowing
  control centers to be added and removed dynamically. The proxy now accepts a
  registry address instead of a fixed control-center list and queries the current
  list whenever discovering vaults. Registry admin transfers require the current
  admin to propose a successor and the proposed admin to accept.
  ([#457](https://github.com/informalsystems/hydro/pull/457))
