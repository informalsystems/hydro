- Add an admin-controlled control center registry for the Inflow proxy, allowing
  control centers to be added and removed dynamically. The proxy now accepts a
  registry address instead of a fixed control-center list and queries the current
  list whenever discovering vaults.
