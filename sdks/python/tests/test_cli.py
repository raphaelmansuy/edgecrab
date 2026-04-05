"""Tests for the edgecrab CLI entry point."""

from __future__ import annotations

import json
import sys
from io import StringIO
from unittest.mock import patch, MagicMock

import httpx
import pytest

from edgecrab.cli import main
from conftest import MOCK_COMPLETION_RESPONSE, MOCK_HEALTH_RESPONSE, MOCK_MODELS_RESPONSE


class TestCliChat:
    """Tests for the CLI chat command."""

    def test_chat_prints_response(self, capsys):
        mock_response = httpx.Response(
            200,
            json=MOCK_COMPLETION_RESPONSE,
            request=httpx.Request("POST", "http://test/v1/chat/completions"),
        )
        with patch("sys.argv", ["edgecrab", "chat", "Hello world"]):
            with patch.object(httpx.Client, "post", return_value=mock_response):
                main()
        captured = capsys.readouterr()
        assert "Hello! I'm EdgeCrab." in captured.out

    def test_chat_no_message_exits(self):
        with patch("sys.argv", ["edgecrab", "chat"]):
            with pytest.raises(SystemExit):
                main()


class TestCliModels:
    """Tests for the CLI models command."""

    def test_models_lists(self, capsys):
        mock_response = httpx.Response(
            200,
            json=MOCK_MODELS_RESPONSE,
            request=httpx.Request("GET", "http://test/v1/models"),
        )
        with patch("sys.argv", ["edgecrab", "models"]):
            with patch.object(httpx.Client, "get", return_value=mock_response):
                main()
        captured = capsys.readouterr()
        assert "claude-sonnet" in captured.out
        assert "gpt-4" in captured.out


class TestCliHealth:
    """Tests for the CLI health command."""

    def test_health_prints_json(self, capsys):
        mock_response = httpx.Response(
            200,
            json=MOCK_HEALTH_RESPONSE,
            request=httpx.Request("GET", "http://test/v1/health"),
        )
        with patch("sys.argv", ["edgecrab", "health"]):
            with patch.object(httpx.Client, "get", return_value=mock_response):
                main()
        captured = capsys.readouterr()
        data = json.loads(captured.out)
        assert data["status"] == "ok"


class TestCliVersion:
    """Tests for the CLI --version flag."""

    def test_version_flag(self, capsys):
        with patch("sys.argv", ["edgecrab", "-V"]):
            with pytest.raises(SystemExit) as exc_info:
                main()
            assert exc_info.value.code == 0
        captured = capsys.readouterr()
        assert "edgecrab-sdk" in captured.out


class TestCliNoCommand:
    """Tests for missing command."""

    def test_no_command_exits(self):
        with patch("sys.argv", ["edgecrab"]):
            with pytest.raises(SystemExit) as exc_info:
                main()
            assert exc_info.value.code == 1
