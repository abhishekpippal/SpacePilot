FREE = {
    "local_core": True,
    "can_use_ai_copilot": False,
    "can_use_full_space_rescue": False,
    "can_use_full_timeline": False,
    "can_use_developer_storage": False,
    "can_use_full_duplicates": False,
}
PRO = {key: True for key in FREE}


def for_plan(plan: str) -> dict:
    return dict(PRO if plan == "PRO" else FREE)
