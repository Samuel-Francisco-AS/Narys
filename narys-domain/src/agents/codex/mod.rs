pub mod app_server;
pub mod backend;
pub mod status;
pub fn planner_probe_response(result: Result<crate::agents::planner::PlanV1, backend::PlannerProbeError>)
  -> Result<crate::agents::planner::PlanV1, String> {
  result.map_err(|error| error.code().to_string())
}

