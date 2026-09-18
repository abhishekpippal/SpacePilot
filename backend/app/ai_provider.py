from abc import ABC, abstractmethod

import httpx
from pydantic import BaseModel, ConfigDict, Field

from .config import get_settings


class Recommendation(BaseModel):
    model_config = ConfigDict(extra="forbid")
    title: str
    explanation: str
    estimated_bytes: int | None = Field(default=None, ge=0)
    risk: str
    source: str


class Action(BaseModel):
    model_config = ConfigDict(extra="forbid")
    type: str
    target_id: str | None = None


class ProviderResult(BaseModel):
    model_config = ConfigDict(extra="forbid")
    summary: str
    facts: list[str]
    recommendations: list[Recommendation]
    warnings: list[str]
    actions: list[Action]
    model: str = Field(exclude=True)
    input_tokens: int = Field(default=0, exclude=True)
    output_tokens: int = Field(default=0, exclude=True)


class ProviderUnavailable(Exception):
    pass


class AiProvider(ABC):
    @abstractmethod
    def generate(self, question: str, context: dict) -> ProviderResult: ...


class OpenAiProvider(AiProvider):
    def generate(self, question: str, context: dict) -> ProviderResult:
        settings = get_settings()
        if not settings.openai_api_key:
            raise ProviderUnavailable("provider is not configured")
        recommendation_fields = {
            "title": {"type": "string"},
            "explanation": {"type": "string"},
            "estimated_bytes": {"type": ["integer", "null"], "minimum": 0},
            "risk": {"type": "string"},
            "source": {"type": "string"},
        }
        action_fields = {
            "type": {"type": "string"},
            "target_id": {"type": ["string", "null"]},
        }
        schema = {
            "type": "object",
            "additionalProperties": False,
            "required": ["summary", "facts", "recommendations", "warnings", "actions"],
            "properties": {
                "summary": {"type": "string"},
                "facts": {"type": "array", "items": {"type": "string"}},
                "recommendations": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "additionalProperties": False,
                        "required": list(recommendation_fields),
                        "properties": recommendation_fields,
                    },
                },
                "warnings": {"type": "array", "items": {"type": "string"}},
                "actions": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "additionalProperties": False,
                        "required": list(action_fields),
                        "properties": action_fields,
                    },
                },
            },
        }
        body = {
            "model": settings.openai_model,
            "store": False,
            "max_output_tokens": 1200,
            "instructions": "Use only supplied aggregate SpacePilot facts. Never invent facts or suggest automatic deletion.",
            "input": [
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "input_text",
                            "text": f"QUESTION:\n{question}\nFACTS:\n{context}",
                        }
                    ],
                }
            ],
            "text": {
                "format": {
                    "type": "json_schema",
                    "name": "spacepilot",
                    "strict": True,
                    "schema": schema,
                }
            },
        }
        try:
            with httpx.Client(timeout=25) as client:
                response = client.post(
                    "https://api.openai.com/v1/responses",
                    headers={"Authorization": f"Bearer {settings.openai_api_key}"},
                    json=body,
                )
                response.raise_for_status()
                data = response.json()
            text = next(
                c["text"]
                for item in data["output"]
                for c in item.get("content", [])
                if c.get("type") == "output_text"
            )
            usage = data.get("usage", {})
            return ProviderResult.model_validate_json(text).model_copy(
                update={
                    "model": settings.openai_model,
                    "input_tokens": usage.get("input_tokens", 0),
                    "output_tokens": usage.get("output_tokens", 0),
                }
            )
        except (httpx.HTTPError, KeyError, StopIteration, ValueError) as exc:
            raise ProviderUnavailable("provider request failed") from exc
