from typing import Any
from pydantic import BaseModel, ConfigDict, EmailStr, Field, field_validator


class Strict(BaseModel):
    model_config = ConfigDict(extra="forbid")


class Signup(Strict):
    email: EmailStr
    password: str = Field(min_length=12, max_length=128)


class Login(Signup):
    pass


class Refresh(Strict):
    refresh_token: str = Field(min_length=32, max_length=256)


class DeviceInput(Strict):
    device_public_id: str = Field(min_length=20, max_length=80)
    device_name: str = Field(min_length=1, max_length=120)
    platform: str = Field(max_length=32)
    architecture: str = Field(max_length=24)
    app_version: str = Field(max_length=24)


class CopilotRequest(Strict):
    app_version: str = Field(max_length=24)
    device_id: str = Field(max_length=80)
    question: str = Field(min_length=1, max_length=1000)
    context: dict[str, Any]

    @field_validator("context")
    @classmethod
    def safe_context(cls, value):
        raw = str(value).lower()
        if len(raw) > 16384 or any(
            x in raw
            for x in [
                "absolute_path",
                "file_content",
                "source_code",
                "sha256",
                "filename",
            ]
        ):
            raise ValueError("unsafe context")
        return value
